import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { request as httpRequest } from 'node:http';
import express from 'express';
import { x402ResourceServer } from '@x402/express';
import { ExactStellarScheme } from '@x402/stellar/exact/server';
import { Address, Keypair, nativeToScVal, xdr } from '@stellar/stellar-sdk';
import { NativeVerifier, QuoteStore, createStellarMerchant, PROFILE } from '../index.mjs';

const filename = process.env.ALLOWIT_STELLAR_HTTP_VERIFIER;
assert.ok(filename, 'Build the native verifier and set ALLOWIT_STELLAR_HTTP_VERIFIER');
const native = new NativeVerifier(path.resolve(filename));
const fixture = JSON.parse(fs.readFileSync(new URL('../../tests/fixtures/x402-vault.json', import.meta.url)));
assert.equal(fixture.fixtureOnly, true);
const now = fixture.prepared.createdAt;
const digest = value => createHash('sha256').update(value).digest();

// Public deterministic SDK test keys. This fixture is never broadcast or used
// on a network. The request hash itself comes from the real Rust helper.
async function fixtureCredential(request) {
  const requestHash = await native.run('request_hash', { request });
  const bytes = value => xdr.ScVal.scvBytes(Buffer.from(value));
  const symbol = value => xdr.ScVal.scvSymbol(value);
  const integer = value => nativeToScVal(BigInt(value), { type: 'u64' });
  const expiresAt = now + 100;
  const preimage = xdr.ScVal.scvVec([symbol('allowit_x402_v1'), new Address(fixture.config.vault).toScVal(),
    bytes(Buffer.from(fixture.prepared.hostDigest, 'hex')), integer(fixture.state.nonce), integer(fixture.state.revision),
    integer(expiresAt), bytes(Buffer.from(requestHash, 'hex'))]);
  const hash = digest(preimage.toXDR());
  const fields = { authority_signature: bytes(Keypair.fromRawEd25519Seed(Buffer.alloc(32, 4)).sign(hash)),
    executor_signature: bytes(Keypair.fromRawEd25519Seed(Buffer.alloc(32, 2)).sign(hash)), expires_at: integer(expiresAt),
    nonce: integer(fixture.state.nonce), request_hash: bytes(Buffer.from(requestHash, 'hex')), revision: integer(fixture.state.revision) };
  const tx = xdr.TransactionEnvelope.fromXDR(fixture.prepared.transactionXdr, 'base64');
  tx.v1().tx().operations()[0].body().invokeHostFunctionOp().auth()[0].credentials().address()
    .signature(xdr.ScVal.scvMap(Object.keys(fields).sort().map(key => new xdr.ScMapEntry({ key: symbol(key), val: fields[key] }))));
  const payload = { x402Version: 2, resource: request.resource, accepted: request.selectedOffer, payload: { transaction: tx.toXDR('base64') } };
  return Buffer.from(JSON.stringify(payload)).toString('base64');
}

function directory() { return fs.mkdtempSync(path.join(os.tmpdir(), 'allowit-stellar-http-')); }

test('persistent quote admission is atomic and never released by restart', async () => {
  const dir = directory();
  const db = path.join(dir, 'quotes.sqlite');
  let a;
  let b;
  try {
    a = new QuoteStore(db);
    b = new QuoteStore(db);
    const key = '11'.repeat(32);
    a.issue(key, { quoteId: '22'.repeat(32), expiresAt: now + 600, request: {} }, now);
    a.claim(key, 'fixture-credential', now);
    assert.throws(() => b.claim(key, 'fixture-credential', now));
    assert.throws(() => b.claim(key, 'different-credential', now));
    a.close(); a = null;
    a = new QuoteStore(db);
    assert.equal(a.get(key).status, 'claimed');
    assert.throws(() => a.claim(key, 'fixture-credential', now));
    assert.throws(() => a.issue(key, { quoteId: '33'.repeat(32), expiresAt: now + 600 }, now));
  } finally { a?.close(); b?.close(); fs.rmSync(dir, { recursive: true }); }
});

test('completed history does not exhaust admission while uncertain claims remain reserved', () => {
  const dir = directory();
  const db = path.join(dir, 'quotes.sqlite');
  let store;
  const first = '11'.repeat(32), second = '22'.repeat(32), third = '33'.repeat(32);
  const record = quoteId => ({ quoteId, expiresAt: now + 600, request: {} });
  try {
    store = new QuoteStore(db, { maxQuotes: 1 });
    store.issue(first, record('44'.repeat(32)), now);
    store.claim(first, 'public-fixture-first', now);
    store.finish(first, { paymentConfirmed: true });
    store.issue(second, record('55'.repeat(32)), now);
    assert.equal(store.get(first).status, 'completed');
    assert.throws(() => store.claim(first, 'public-fixture-first', now));
    store.claim(second, 'public-fixture-second', now);
    assert.throws(() => store.issue(third, record('66'.repeat(32)), now + 601));
    store.finish(second, { paymentConfirmed: false });
    store.close();
    store = new QuoteStore(db, { maxQuotes: 1 });
    assert.equal(store.get(first).status, 'completed');
    assert.equal(store.get(second).status, 'uncertain');
    assert.throws(() => store.claim(second, 'public-fixture-second', now));
    assert.throws(() => store.issue(third, record('66'.repeat(32)), now + 601));
  } finally { store?.close(); fs.rmSync(dir, { recursive: true }); }
});

test('native helper uses the original shared request hash and public authorization', async () => {
  assert.equal(await native.run('request_hash', { request: fixture.prepared.request }), fixture.prepared.requestHash);
  const result = await native.run('inspect', { credential: fixture.signed.paymentSignature });
  assert.equal(result.requestHash, fixture.prepared.requestHash);
  assert.equal(result.vault, fixture.config.vault);
  await assert.rejects(native.run('inspect', { credential: 'invalid' }));
});

async function merchantFixture() {
  const dir = directory();
  const store = new QuoteStore(path.join(dir, 'quotes.sqlite'));
  const calls = { verify: 0, settle: 0, handler: 0, state: 0 };
  const rejections = [];
  const client = {
    async getSupported() { return { kinds: [{ x402Version: 2, scheme: 'exact', network: 'stellar:testnet', extra: { areFeesSponsored: true } }], extensions: [], signers: {} }; },
    async verify() { calls.verify++; return { isValid: true, payer: fixture.config.vault }; },
    async settle() { calls.settle++; return { success: true, transaction: '33'.repeat(32), network: 'stellar:testnet', payer: fixture.config.vault }; },
  };
  const server = new x402ResourceServer(client).register('stellar:testnet', new ExactStellarScheme());
  const app = express();
  app.set('trust proxy', 'loopback');
  const route = action => ({ method: 'GET', path: action === 'hash' ? '/paid' : '/other', action, amount: '10000', handler: async request => {
    assert.equal(calls.settle > 0, true, 'Settlement precedes handler');
    assert.equal(Object.hasOwn(request, 'headers'), false);
    calls.handler++;
    return { status: 200, body: { fixtureOnly: true, action: request.action, url: request.url } };
  } });
  app.use(await createStellarMerchant({ origin: 'https://merchant.example', merchant: 'fixture-merchant',
    network: 'stellar:testnet', payTo: fixture.selectedOffer.payTo, routes: [route('hash'), route('other')],
    native, store, resourceServer: server, attestationSigner: Keypair.fromRawEd25519Seed(Buffer.alloc(32, 8)), now: () => now,
    onRejected: event => rejections.push(event.reason),
    loadVault: async () => { calls.state++; return { config: fixture.config, state: fixture.state, latestLedger: 3444 }; } }));
  app.use((_req, res) => res.status(404).json({ error: 'Unknown resource' }));
  const listener = await new Promise(resolve => { const value = app.listen(0, '127.0.0.1', () => resolve(value)); });
  const local = 'http://127.0.0.1:' + listener.address().port;
  async function request(target, options = {}) {
    return await new Promise((resolve, reject) => {
      const outgoing = httpRequest(local + target, { method: options.method ?? 'GET',
        headers: { Host: 'merchant.example', 'X-Forwarded-Proto': 'https', ...(options.headers ?? {}) } }, response => {
        const parts = [];
        response.on('data', bytes => parts.push(bytes));
        response.on('error', reject);
        response.on('end', () => {
          try { resolve({ status: response.statusCode, headers: new Headers(response.headers),
            body: JSON.parse(Buffer.concat(parts).toString()) }); } catch (error) { reject(error); }
        });
      });
      outgoing.on('error', reject);
      outgoing.end(options.body);
    });
  }
  async function quote() {
    const response = await request('/paid?text=hi');
    assert.equal(response.status, 402, rejections.join('; '));
    const required = JSON.parse(Buffer.from(response.headers.get('payment-required'), 'base64').toString());
    const original = { method: 'GET', url: 'https://merchant.example/paid?text=hi', body: [], resource: required.resource,
      paymentRequired: required, selectedOffer: required.accepts[0] };
    return { original, credential: await fixtureCredential(original) };
  }
  async function close() { await new Promise(resolve => listener.close(resolve)); store.close(); fs.rmSync(dir, { recursive: true }); }
  return { request, quote, calls, store, close };
}

test('original request settles once through ignored transport metadata and consumes its persistent quote', async () => {
  const m = await merchantFixture();
  try {
    const { original, credential } = await m.quote();
    const result = await m.request('/paid?text=hi', { headers: { 'Payment-Signature': credential,
      'CF-EW-Via': '15', 'CF-Warp-Tag-Id': 'fixture-transport', 'CF-Worker': 'fixture.example' } });
    assert.equal(result.status, 200);
    assert.deepEqual(m.calls, { verify: 0, settle: 1, handler: 1, state: 1 });
    const hash = await native.run('request_hash', { request: original });
    assert.equal(m.store.get(hash).status, 'completed');
    const repeated = await m.request('/paid?text=hi', { headers: { 'Payment-Signature': credential } });
    assert.equal(repeated.status, 403);
    assert.equal(m.calls.settle, 1);
    assert.equal(m.calls.handler, 1);
  } finally { await m.close(); }
});

test('request mismatches stop before state, settlement or handler and preserve the valid quote', async () => {
  const m = await merchantFixture();
  try {
    const { credential } = await m.quote();
    for (const target of ['/other?text=hi', '/paid?text=other', '/paid?text=hi&extra=1', '/%70aid?text=hi', '/unknown']) {
      const result = await m.request(target, { headers: { 'Payment-Signature': credential } });
      assert.equal(result.status, 403, target);
    }
    for (const headers of [{ Host: 'other.example' }, { Cookie: 'tenant=other' }, { 'X-Action': 'other' }, { 'X-Forwarded-Host': 'other.example' }, { 'X-Payment': credential }]) {
      const result = await m.request('/paid?text=hi', { headers: { 'Payment-Signature': credential, ...headers } });
      assert.equal(result.status, 403);
    }
    assert.deepEqual(m.calls, { verify: 0, settle: 0, handler: 0, state: 0 });
    const valid = await m.request('/paid?text=hi', { headers: { 'Payment-Signature': credential } });
    assert.equal(valid.status, 200);
    assert.equal(m.calls.settle, 1);
  } finally { await m.close(); }
});

test('engine probe retrieves exact original challenge with a signed fresh nonce', async () => {
  const m = await merchantFixture();
  try {
    const { original } = await m.quote();
    const requestHash = await native.run('request_hash', { request: original });
    const nonce = '44'.repeat(32);
    const result = await m.request('/paid?text=hi', { headers: { 'X-AllowIt-Request-Hash': requestHash, 'X-AllowIt-Binding-Nonce': nonce } });
    assert.equal(result.status, 402);
    assert.deepEqual(result.body, original.paymentRequired);
    const binding = JSON.parse(Buffer.from(result.headers.get('x-allowit-request-binding'), 'base64').toString());
    assert.equal(binding.claims.profile, PROFILE);
    assert.equal(binding.claims.requestHash, requestHash);
    assert.equal(binding.claims.nonce, nonce);
    assert.equal(binding.claims.verifierHash, native.verifierHash);
    const hash = await native.run('attestation_digest', { claims: binding.claims });
    assert.equal(Keypair.fromPublicKey(original.selectedOffer.extra.allowitHttpBinding.verificationKey)
      .verify(Buffer.from(hash, 'hex'), Buffer.from(binding.signature, 'hex')), true);
    assert.equal(m.store.get(requestHash).status, 'issued');
    const wrong = await m.request('/paid?text=other', { headers: { 'X-AllowIt-Request-Hash': requestHash, 'X-AllowIt-Binding-Nonce': nonce } });
    assert.equal(wrong.status, 403);
    assert.deepEqual(m.calls, { verify: 0, settle: 0, handler: 0, state: 0 });
  } finally { await m.close(); }
});
