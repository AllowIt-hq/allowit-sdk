import { createHash, randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { paymentMiddleware } from '@x402/express';
import { Account, Contract, Networks, StrKey, TransactionBuilder } from '@stellar/stellar-sdk';

export const PROFILE = 'allowit-stellar-http-binding-v1';
const MAX_BODY = 1_048_576;
const MAX_CREDENTIAL = 2_097_152;
const MAX_RPC = 2_097_152;
const HEX = /^[a-f0-9]{64}$/;
const ASSETS = Object.freeze({
  'stellar:testnet': 'CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA',
  'stellar:pubnet': 'CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75',
});
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const seconds = () => Math.floor(Date.now() / 1000);

function requireValue(condition, message) {
  if (!condition) throw new Error(message);
}

/** A fixed executable selected by the merchant operator, never by the caller. */
export class NativeVerifier {
  constructor(filename) {
    requireValue(path.isAbsolute(filename), 'An absolute native verifier path is required');
    this.filename = filename;
    this.verifierHash = this.hash();
  }
  hash() {
    const info = fs.lstatSync(this.filename);
    requireValue(info.isFile() && !info.isSymbolicLink() && info.size < 268_435_456, 'Invalid verifier executable');
    if (process.platform !== 'win32') requireValue((info.mode & 0o022) === 0, 'Verifier must not be writable by other users');
    return sha(fs.readFileSync(this.filename));
  }
  async run(op, fields) {
    requireValue(this.hash() === this.verifierHash, 'The native verifier changed');
    const input = JSON.stringify({ op, ...fields }) + '\n';
    requireValue(Buffer.byteLength(input) <= 8_388_608, 'Verifier input exceeds bounds');
    return await new Promise((resolve, reject) => {
      const child = spawn(this.filename, [], {
        stdio: ['pipe', 'pipe', 'ignore'],
        env: { PATH: process.env.PATH ?? '', SystemRoot: process.env.SystemRoot ?? '' },
        windowsHide: true,
      });
      let output = '';
      let finished = false;
      const finish = (error, value) => {
        if (finished) return;
        finished = true;
        clearTimeout(timer);
        error ? reject(error) : resolve(value);
      };
      const timer = setTimeout(() => { child.kill(); finish(new Error('Native verifier timed out')); }, 10_000);
      child.on('error', () => finish(new Error('Native verifier unavailable')));
      child.stdin.on('error', () => finish(new Error('Native verifier input unavailable')));
      child.stdout.on('data', bytes => {
        output += bytes.toString('utf8');
        if (Buffer.byteLength(output) > MAX_RPC) { child.kill(); finish(new Error('Native verifier output exceeds bounds')); }
      });
      child.on('close', code => {
        try {
          requireValue(code === 0, 'Native verifier failed');
          const response = JSON.parse(output);
          requireValue(response.ok === true && Object.hasOwn(response, 'result'), 'Native verification rejected the request');
          finish(null, response.result);
        } catch (error) { finish(error); }
      });
      child.stdin.end(input);
    });
  }
}

/** Durable quote identity and at-most-once handler admission, across processes. */
export class QuoteStore {
  constructor(filename, { maxQuotes = 10_000 } = {}) {
    requireValue(path.isAbsolute(filename) && filename !== ':memory:', 'A persistent absolute quote database path is required');
    requireValue(!filename.startsWith('/mnt/'), 'Private quote storage requires a POSIX filesystem');
    requireValue(Number.isSafeInteger(maxQuotes) && maxQuotes > 0 && maxQuotes <= 10_000, 'Invalid quote quota');
    const directory = path.dirname(filename);
    fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
    const parent = fs.lstatSync(directory);
    requireValue(parent.isDirectory() && !parent.isSymbolicLink(), 'Invalid quote directory');
    if (process.platform !== 'win32') {
      requireValue((parent.mode & 0o077) === 0 && parent.uid === process.getuid(), 'Quote directory must be private');
    }
    if (fs.existsSync(filename)) {
      const info = fs.lstatSync(filename);
      requireValue(info.isFile() && !info.isSymbolicLink(), 'Invalid quote database');
      if (process.platform !== 'win32') requireValue((info.mode & 0o077) === 0 && info.uid === process.getuid(), 'Quote database must be private');
    } else fs.closeSync(fs.openSync(filename, 'wx', 0o600));
    this.maxQuotes = maxQuotes;
    this.db = new DatabaseSync(filename);
    this.db.exec(`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=10000;
      CREATE TABLE IF NOT EXISTS quotes (
        request_hash TEXT PRIMARY KEY, quote_id TEXT NOT NULL UNIQUE,
        expires_at INTEGER NOT NULL, record TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'issued', credential_digest TEXT,
        claimed_at INTEGER, outcome TEXT
      ) STRICT;`);
  }
  issue(requestHash, record, now) {
    requireValue(HEX.test(requestHash), 'Invalid request hash');
    const json = JSON.stringify(record);
    requireValue(Buffer.byteLength(json) <= MAX_RPC, 'Quote exceeds bounds');
    this.db.exec('BEGIN IMMEDIATE');
    try {
      // Never remove a claimed/uncertain operation. An unused expired quote is
      // unusable even if the executor retained a valid signature over it.
      this.db.prepare("DELETE FROM quotes WHERE status='issued' AND expires_at <= ?").run(now);
      const count = this.db.prepare('SELECT count(*) AS n FROM quotes').get().n;
      requireValue(count < this.maxQuotes, 'Quote quota exhausted');
      this.db.prepare('INSERT INTO quotes(request_hash,quote_id,expires_at,record) VALUES(?,?,?,?)')
        .run(requestHash, record.quoteId, record.expiresAt, json);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }
  get(requestHash) {
    requireValue(HEX.test(requestHash), 'Invalid request hash');
    const row = this.db.prepare('SELECT * FROM quotes WHERE request_hash=?').get(requestHash);
    requireValue(row, 'The original quote is unavailable');
    return { ...row, ...JSON.parse(row.record) };
  }
  claim(requestHash, credential, now) {
    const result = this.db.prepare("UPDATE quotes SET status='claimed',credential_digest=?,claimed_at=? WHERE request_hash=? AND status='issued' AND expires_at>?")
      .run(sha(credential), now, requestHash, now);
    requireValue(result.changes === 1, 'The original quote is already claimed or expired');
  }
  finish(requestHash, outcome) {
    this.db.prepare("UPDATE quotes SET status=?,outcome=? WHERE request_hash=? AND status='claimed'")
      .run(outcome.paymentConfirmed ? 'completed' : 'uncertain', JSON.stringify(outcome), requestHash);
  }
  close() { this.db.close(); }
}

async function boundedJson(response) {
  requireValue(response.ok, 'Configured RPC unavailable');
  let size = 0;
  const parts = [];
  for await (const chunk of response.body) {
    size += chunk.length;
    requireValue(size <= MAX_RPC, 'RPC response exceeds bounds');
    parts.push(chunk);
  }
  return JSON.parse(Buffer.concat(parts).toString('utf8'));
}

/** Resolve current vault state and immutable pins only through a fixed RPC. */
export function createRpcVaultLoader({ native, network, rpcUrl, rpcHosts, vaultWasmHash, policyWasmHash, policySourceHash, policy }) {
  const endpoint = new URL(rpcUrl);
  requireValue(ASSETS[network] && endpoint.protocol === 'https:' && !endpoint.username && !endpoint.password && !endpoint.hash && !endpoint.search,
    'A configured canonical Stellar HTTPS RPC is required');
  const allowed = rpcHosts ?? (network === 'stellar:testnet' ? ['soroban-testnet.stellar.org'] : ['soroban-rpc.mainnet.stellar.gateway.fm']);
  requireValue(allowed.includes(endpoint.hostname) && (!endpoint.port || endpoint.port === '443'), 'RPC host is not configured');
  for (const pin of [vaultWasmHash, policyWasmHash, policySourceHash]) requireValue(HEX.test(pin), 'Invalid immutable deployment pin');
  let sequence = 0;
  const passphrase = network === 'stellar:testnet' ? Networks.TESTNET : Networks.PUBLIC;
  async function rpc(method, params) {
    const id = ++sequence;
    const response = await fetch(rpcUrl, { method: 'POST', redirect: 'error', signal: AbortSignal.timeout(10_000),
      headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id, method, params }) });
    const json = await boundedJson(response);
    requireValue(json.jsonrpc === '2.0' && json.id === id && !json.error && Object.hasOwn(json, 'result'), 'Invalid configured RPC response');
    return json.result;
  }
  return async vault => {
    requireValue(typeof vault === 'string' && StrKey.isValidContract(vault), 'Invalid vault identity');
    const networkInfo = await rpc('getNetwork', {});
    requireValue(networkInfo.passphrase === passphrase, 'Configured network changed');
    const transaction = new TransactionBuilder(new Account(StrKey.encodeEd25519PublicKey(Buffer.alloc(32)), '0'), { fee: '100', networkPassphrase: passphrase })
      .addOperation(new Contract(vault).call('state')).setTimeout(0).build().toXDR();
    const simulation = await rpc('simulateTransaction', { transaction });
    requireValue(!simulation.error && simulation.results?.length === 1, 'Vault state unavailable');
    const state = await native.run('parse_state', { resultXdr: simulation.results[0].xdr });
    requireValue(state.policy === policy, 'Unknown numeric policy deployment');
    const config = { network, rpcUrl, vault, owner: state.owner, executor: state.executor, authority: state.authority,
      asset: ASSETS[network], policy, vaultWasmHash, policyWasmHash, policySourceHash };
    const keys = await native.run('ledger_keys', { config });
    const entries = await rpc('getLedgerEntries', { keys });
    await native.run('validate_entries', { config, entries: entries.entries });
    const latest = await rpc('getLatestLedger', {});
    requireValue(Number.isSafeInteger(latest.sequence) && latest.sequence > 0 &&
      Number.isSafeInteger(simulation.latestLedger) && Number.isSafeInteger(entries.latestLedger) &&
      latest.sequence >= Math.max(simulation.latestLedger, entries.latestLedger) &&
      latest.sequence - simulation.latestLedger <= 2 && latest.sequence - entries.latestLedger <= 2, 'Vault state is stale');
    return { config, state, latestLedger: latest.sequence };
  };
}

const ALLOWED_HEADERS = new Set(['host', 'payment-signature', 'x-allowit-request-hash', 'x-allowit-binding-nonce',
  'accept', 'accept-language', 'sec-fetch-mode', 'user-agent', 'accept-encoding', 'connection', 'content-length', 'transfer-encoding', 'content-type',
  'idempotency-key', 'x-forwarded-proto', 'x-forwarded-for', 'cf-connecting-ip', 'cf-ray', 'cf-ipcountry',
  'cf-visitor', 'cdn-loop', 'via']);

function incoming(req, origin, route) {
  requireValue(req.protocol === 'https' && req.headers.host === new URL(origin).host, 'Unexpected merchant ingress');
  const counts = new Map();
  for (let i = 0; i < req.rawHeaders.length; i += 2) {
    const key = req.rawHeaders[i].toLowerCase();
    counts.set(key, (counts.get(key) ?? 0) + 1);
    requireValue(ALLOWED_HEADERS.has(key), 'Unbound request header is not supported');
  }
  for (const key of ['host', 'payment-signature', 'x-allowit-request-hash', 'x-allowit-binding-nonce', 'content-type', 'content-length', 'transfer-encoding'])
    requireValue((counts.get(key) ?? 0) <= 1, 'Ambiguous HTTP header');
  requireValue(!req.headers['content-encoding'] && !req.headers['x-payment'], 'Unsupported request encoding');
  const target = req.originalUrl;
  requireValue(typeof target === 'string' && target.startsWith('/') && !target.startsWith('//') &&
    !target.includes('#') && !/[\x00-\x20\x7f]/.test(target) && !/%(?![a-fA-F0-9]{2})/.test(target) && req.url === target,
    'Ambiguous HTTP request target');
  const parsed = new URL(target, origin);
  requireValue(parsed.href === origin + target && parsed.pathname === route.path && req.method === route.method, 'Request target changed');
  const contentType = req.headers['content-type'];
  if (['GET', 'HEAD'].includes(req.method)) requireValue(!contentType && !req.headers['transfer-encoding'] && Number(req.headers['content-length'] ?? 0) === 0, 'GET body is not supported');
  else requireValue(contentType === route.contentType, 'The configured entity content type is required');
  return { method: req.method, url: origin + target };
}

async function rawEntity(req) {
  requireValue(req.body === undefined && !req.readableEnded, 'The guard must run before body parsers');
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    requireValue(size <= MAX_BODY, 'Request body exceeds bounds');
    chunks.push(chunk);
  }
  return [...Buffer.concat(chunks)];
}

function marker(record) { return record.request.selectedOffer.extra?.allowitHttpBinding; }

/**
 * Install globally before parsers and other paid handlers, after trusted proxy
 * configuration. Handlers receive only the verified descriptor, never mutable
 * headers/cookies or an unsigned request context. All errors fail closed.
 */
export async function createStellarMerchant({ origin, merchant, network, payTo, routes, native, store, resourceServer,
  attestationSigner, loadVault, quoteSeconds = 600, now = seconds, onRejected = () => {} }) {
  requireValue(new URL(origin).origin === origin && origin.startsWith('https://') && !new URL(origin).port, 'A canonical public HTTPS origin is required');
  requireValue(typeof merchant === 'string' && /^[a-zA-Z0-9_-]{1,128}$/.test(merchant), 'Invalid merchant identity');
  requireValue(ASSETS[network] && StrKey.isValidEd25519PublicKey(payTo), 'Canonical Stellar merchant configuration is required');
  requireValue(native instanceof NativeVerifier && store instanceof QuoteStore && typeof loadVault === 'function', 'Native verifier, persistent store and trusted state loader are required');
  requireValue(Number.isSafeInteger(quoteSeconds) && quoteSeconds > 0 && quoteSeconds <= 600, 'Invalid quote lifetime');
  const verificationKey = attestationSigner.publicKey();
  requireValue(StrKey.isValidEd25519PublicKey(verificationKey), 'Invalid binding attestation key');
  const table = new Map();
  for (const route of routes) {
    requireValue(['GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE'].includes(route.method) && typeof route.path === 'string' &&
      route.path.startsWith('/') && !route.path.includes('?') && !route.path.includes('%') &&
      /^[a-zA-Z0-9_-]{1,128}$/.test(route.action) && typeof route.handler === 'function' &&
      /^[1-9][0-9]*$/.test(route.amount) && BigInt(route.amount) % 10n === 0n && BigInt(route.amount) <= 18_446_744_073_709_551_615n,
      'Invalid exact merchant resource');
    if (!['GET', 'HEAD'].includes(route.method)) requireValue(route.contentType === 'application/octet-stream', 'Body resources require a fixed raw entity type');
    const key = route.method + ' ' + route.path;
    requireValue(!table.has(key), 'Duplicate merchant resource');
    table.set(key, Object.freeze({ ...route }));
  }
  await resourceServer.initialize();
  function match(record, route, actual, time) {
    const value = marker(record);
    requireValue(value && value.profile === PROFILE && value.verifierHash === native.verifierHash &&
      value.verificationKey === verificationKey && value.merchant === merchant && value.action === route.action &&
      value.quoteId === record.quoteId && value.expiresAt === record.expiresAt && value.expiresAt > time &&
      value.issuedAt <= time && value.expiresAt <= value.issuedAt + 600 &&
      record.merchant === merchant && record.action === route.action &&
      record.request.method === actual.method && record.request.url === actual.url &&
      JSON.stringify(record.request.body) === JSON.stringify(actual.body) &&
      record.request.selectedOffer.network === network && record.request.selectedOffer.payTo === payTo &&
      record.request.selectedOffer.asset === ASSETS[network] && record.request.selectedOffer.amount === route.amount &&
      record.request.selectedOffer.extra.paymentFlow === 'upfront', 'The original resource binding no longer matches');
  }
  function challenge(res, required, binding) {
    res.setHeader('PAYMENT-REQUIRED', Buffer.from(JSON.stringify(required)).toString('base64'));
    res.setHeader('Cache-Control', 'private, no-store');
    if (binding) res.setHeader('X-AllowIt-Request-Binding', Buffer.from(JSON.stringify(binding)).toString('base64'));
    res.status(402).json(required);
  }
  return async function stellarRequestBoundary(req, res, next) {
    let claimedHash;
    try {
      const rawPath = typeof req.originalUrl === 'string' ? req.originalUrl.split('?')[0] : '';
      const route = table.get(req.method + ' ' + rawPath);
      if (!route) {
        if (req.headers['payment-signature'] || req.headers['x-payment'] || req.headers['x-allowit-request-hash']) {
          res.status(403).json({ error: 'This resource has no verified Stellar request binding' }); return;
        }
        return next();
      }
      const descriptor = incoming(req, origin, route);
      const actual = { ...descriptor, body: await rawEntity(req) };
      const time = now();
      const credential = req.headers['payment-signature'];
      const requestedHash = req.headers['x-allowit-request-hash'];
      if (!credential && requestedHash) {
        const nonce = req.headers['x-allowit-binding-nonce'];
        requireValue(HEX.test(requestedHash) && HEX.test(nonce ?? ''), 'A bounded engine binding nonce is required');
        const record = store.get(requestedHash);
        requireValue(record.status === 'issued', 'The quote is already claimed');
        match(record, route, actual, time);
        const challengeHash = await native.run('canonical_hash', { value: record.request.paymentRequired });
        const claims = { profile: PROFILE, merchant, action: route.action, origin, requestHash: requestedHash,
          challengeHash, nonce, issuedAt: time, expiresAt: Math.min(time + 30, record.expiresAt), verifierHash: native.verifierHash };
        const digest = await native.run('attestation_digest', { claims });
        const signature = attestationSigner.sign(Buffer.from(digest, 'hex')).toString('hex');
        requireValue(now() < claims.expiresAt, 'Binding proof expired during construction');
        challenge(res, record.request.paymentRequired, { claims, signature }); return;
      }
      if (!credential) {
        requireValue(!req.headers['x-allowit-binding-nonce'], 'Binding nonce requires the original request hash');
        const quoteId = randomBytes(32).toString('hex');
        const expiresAt = time + quoteSeconds;
        const extra = { paymentFlow: 'upfront', allowitHttpBinding: { profile: PROFILE, quoteId, issuedAt: time, expiresAt,
          merchant, action: route.action, verificationKey, verifierHash: native.verifierHash } };
        const accepts = await resourceServer.buildPaymentRequirements({ scheme: 'exact', network, payTo,
          price: { asset: ASSETS[network], amount: route.amount, extra }, maxTimeoutSeconds: 300 });
        requireValue(accepts.length === 1 && accepts[0].extra.areFeesSponsored === true, 'The sponsored facilitator profile is required');
        const resource = { url: actual.url, description: route.description ?? route.action, mimeType: 'application/json' };
        const required = await resourceServer.createPaymentRequiredResponse(accepts, resource, 'Payment required');
        requireValue(required.accepts.length === 1 && required.accepts[0].extra.allowitHttpBinding.quoteId === quoteId,
          'The original quote changed during enrichment');
        const request = { ...actual, resource: required.resource, paymentRequired: required, selectedOffer: required.accepts[0] };
        const requestHash = await native.run('request_hash', { request });
        store.issue(requestHash, { merchant, action: route.action, quoteId, expiresAt, request }, time);
        challenge(res, required); return;
      }
      requireValue(typeof credential === 'string' && credential.length <= MAX_CREDENTIAL && !requestedHash &&
        !req.headers['x-allowit-binding-nonce'], 'Invalid payment transport');
      const authorization = await native.run('inspect', { credential });
      const record = store.get(authorization.requestHash);
      match(record, route, actual, time);
      const current = await loadVault(authorization.vault);
      requireValue(current.config.network === network && current.config.vault === authorization.vault, 'State loader returned a different vault');
      await native.run('verify', { ...current, request: record.request, actual, credential, now: now() });
      // Claim only after local request/signature/state validation, but before
      // any facilitator call or handler. Never release an attempted claim.
      claimedHash = authorization.requestHash;
      store.claim(claimedHash, credential, now());
      res.once('finish', () => {
        try {
          const raw = res.getHeader('PAYMENT-RESPONSE');
          const receipt = typeof raw === 'string' ? JSON.parse(Buffer.from(raw, 'base64').toString('utf8')) : null;
          store.finish(claimedHash, { status: res.statusCode, paymentConfirmed: receipt?.success === true && HEX.test(receipt.transaction ?? '') && receipt.network === network,
            transaction: receipt?.transaction ?? null, delivered: res.statusCode >= 200 && res.statusCode < 300 });
        } catch { store.finish(claimedHash, { paymentConfirmed: false, delivered: false, status: res.statusCode }); }
      });
      const offer = record.request.selectedOffer;
      const config = { accepts: [{ scheme: offer.scheme, network: offer.network, payTo: offer.payTo,
        price: { asset: offer.asset, amount: offer.amount, extra: offer.extra }, maxTimeoutSeconds: offer.maxTimeoutSeconds }],
        resource: record.request.resource.url, description: record.request.resource.description, mimeType: record.request.resource.mimeType };
      const middleware = paymentMiddleware({ [route.method + ' ' + route.path]: config }, resourceServer);
      await middleware(req, res, async () => {
        // The upfront published flow confirmed settlement before dispatch.
        // Do not expose headers/cookies or mutable Express routing to handlers.
        try {
          const response = await route.handler(Object.freeze({ ...actual, body: Uint8Array.from(actual.body),
            action: route.action, merchant, requestHash: claimedHash }));
          requireValue(response && Number.isInteger(response.status) && response.status >= 200 && response.status < 300 &&
            Buffer.byteLength(JSON.stringify(response.body)) <= MAX_BODY, 'Invalid bounded API delivery');
          res.status(response.status).json(response.body);
        } catch { res.status(502).json({ error: 'Payment handler delivery unavailable' }); }
      });
    } catch (error) {
      try { onRejected({ reason: String(error?.message ?? 'Binding validation failed').slice(0, 256) }); } catch { /* telemetry cannot permit payment */ }
      if (claimedHash) {
        try { store.finish(claimedHash, { paymentConfirmed: false, delivered: false, status: 503 }); } catch { /* retained claim */ }
      }
      if (!res.headersSent) res.status(403).json({ error: 'Stellar request binding verification failed' });
      else res.destroy();
    }
  };
}
