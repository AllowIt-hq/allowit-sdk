import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { generateKeyPairSync } from 'node:crypto';
import { createKeyPairFromBytes, getAddressDecoder, getTransactionDecoder, signTransaction, getBase64EncodedWireTransaction } from '@solana/kit';
import { Driver } from '../driver.mjs';
import { durableJSON, readJSON, sha256, NETWORK } from '../safety.mjs';

async function fixture(action) {
  const directory = await mkdtemp(join(tmpdir(), 'allowit-driver-'));
  try {
    const driver = new Driver({ privateDirectory: directory, programId: 'program' });
    driver.keys = { executor: { address: 'executor' }, evidence: { address: 'evidence' }, compiler: { address: 'compiler' } };
    const record = { owner: 'owner', policyId: '01'.repeat(32), sourceHash: 's', irHash: 'i', stateAddress: 'state', sourceTokenAccount: 'source', destinationTokenAccount: 'destination', mandate: { recipient: 'recipient', expiresAt: '9999999999' } };
    driver.record = async () => record;
    driver.assertNetwork = async () => {};
    driver.fetchedState = async () => ({ state: { nextNonce: '0', spentUnits: '0', mandate: record.mandate }, stateBase64: 'fixture' });
    const body = { owner: record.owner, policyId: record.policyId, requestId: 'request-0001', amountUnits: '1', context: {}, semanticEvidence: {} };
    await action(driver, body, record, directory);
  } finally { await rm(directory, { recursive: true, force: true }); }
}
test('numeric empty semantic object is absent; rejected preflight is cached and changed ID reuse fails', async () => {
  await fixture(async (driver, body) => {
    let calls = 0;
    driver.codec = async input => { calls++; assert.equal(input.evidence, undefined); return { requestHash: 'bound', nonce: '0', contextHash: 'context', runtimeContext: '{}', preflight: { allowed: false, status: 'denied', error: 'BudgetExceeded' } }; };
    driver.submit = async () => { throw new Error('a denied request must not be sent'); };
    const first = await driver.execute(body); assert.equal(first.status, 'denied'); assert.equal(first.chainVerified, false);
    assert.deepEqual(await driver.execute(body), first); assert.equal(calls, 1);
    await assert.rejects(driver.execute({ ...body, amountUnits: '2' }), /different request details/);
  });
});
test('a durable unknown outcome cannot trigger a second submission', async () => {
  await fixture(async (driver, body, record, directory) => {
    const path = join(directory, 'requests', `${sha256(`${record.owner}\n${record.policyId}\n${body.requestId}`)}.json`);
    await durableJSON(path, { inputHash: sha256(JSON.stringify(body)), signature: 'already-signed', status: 'submission_uncertain' }, true);
    driver.codec = async () => { throw new Error('unknown requests must not be reconstructed'); };
    driver.submit = async () => { throw new Error('unknown requests must not be sent'); };
    const result = await driver.execute(body);
    assert.equal(result.status, 'unknown'); assert.equal(result.signature, 'already-signed'); assert.equal(result.chainVerified, false);
  });
});
test('submission uncertainty returns the bound known signature without caching a terminal result or resending', async () => {
  await fixture(async (driver, body, record, directory) => {
    let sends = 0;
    const signature = '1'.repeat(88);
    driver.codec = async () => ({ requestHash: 'bound', nonce: '0', runtimeContext: '{}', preflight: { allowed: true }, computeBudgetInstructions: [], instruction: {} });
    driver.build = async () => ({ transaction: 'wire' });
    driver.submit = async (_, __, id) => {
      sends++;
      await durableJSON(join(directory, 'transactions', `${id}.json`), { signature, wire: 'immutable-signed-wire', status: 'submitted', lastValidBlockHeight: '100' });
      throw new Error(`transaction_expired:${signature}`);
    };
    const result = await driver.execute(body);
    assert.equal(result.status, 'unknown'); assert.equal(result.chainVerified, false); assert.equal(result.signature, signature);
    for (const field of ['owner', 'policyId', 'sourceHash', 'irHash']) assert.equal(result[field], record[field]);
    assert.equal(result.lastValidBlockHeight, '100'); assert(result.explorerUrl.includes(signature));
    const path = join(directory, 'requests', `${sha256(`${record.owner}\n${record.policyId}\n${body.requestId}`)}.json`);
    const intent = await readJSON(path); assert.equal(intent.result, undefined); assert.equal(intent.signature, signature);
    const retry = await driver.execute(body); assert.equal(retry.status, 'unknown'); assert.equal(retry.signature, signature); assert.equal(sends, 1);
  });
});
test('pending receipt rows preserve full owner and artifact binding, with a signature only when known', async () => {
  await fixture(async (driver, body, record, directory) => {
    const base = { ...driver.binding(record), state: { nextNonce: '0', spentUnits: '0' } };
    const intent = { owner: record.owner, policyId: record.policyId, requestId: body.requestId, requestHash: 'bound', transactionId: 'pending', base };
    await durableJSON(join(directory, 'requests', `${sha256('pending')}.json`), intent);
    await durableJSON(join(directory, 'transactions', 'pending.json'), { signature: '1'.repeat(88), wire: 'immutable', lastValidBlockHeight: '100' });
    await durableJSON(join(directory, 'requests', `${sha256('no-journal')}.json`), { ...intent, requestId: 'no-journal', transactionId: 'absent' });
    driver.rpc = async method => { assert.equal(method, 'getSignatureStatuses'); return { value: [{ confirmationStatus: 'confirmed' }] }; };
    const results = await driver.requestReceipts(record, base.state);
    for (const result of results) {
      assert.equal(result.status, 'unknown'); assert.equal(result.chainVerified, false);
      for (const field of ['owner', 'policyId', 'sourceHash', 'irHash']) assert.equal(result[field], record[field]);
    }
    assert.equal(results.find(result => result.requestId === body.requestId).signature, '1'.repeat(88));
    assert.equal(Object.hasOwn(results.find(result => result.requestId === 'no-journal'), 'signature'), false);
  });
});
test('absence audit requires finalized expiry, null history and transaction, and unchanged fresh counters', async () => {
  await fixture(async (driver, body, record) => {
    const intent = { requestId: body.requestId, base: { ...driver.binding(record), state: { nextNonce: '0', spentUnits: '0' } } };
    const journal = { signature: '1'.repeat(88), lastValidBlockHeight: '100', wire: 'immutable' };
    let height = 100, history = null, transaction = null, stateReads = 0, transactionReads = 0;
    driver.rpc = async (method, params) => {
      if (method === 'getBlockHeight') { assert.deepEqual(params, [{ commitment: 'finalized' }]); return height; }
      if (method === 'getSignatureStatuses') { assert.equal(params[1].searchTransactionHistory, true); return { value: [history] }; }
      assert.equal(method, 'getTransaction'); assert.equal(params[1].commitment, 'finalized'); transactionReads++; return transaction;
    };
    driver.fetchedState = async () => { stateReads++; return { state: { nextNonce: '1', spentUnits: '1' } }; };
    assert.equal(await driver.expiredReceipt(record, intent, journal), null); assert.equal(transactionReads, 0);
    height = 101; history = { confirmationStatus: 'confirmed' };
    assert.equal(await driver.expiredReceipt(record, intent, journal), null); assert.equal(transactionReads, 0);
    history = null; transaction = { transaction: ['immutable', 'base64'] };
    assert.equal(await driver.expiredReceipt(record, intent, journal), null); assert.equal(stateReads, 0);
    transaction = null;
    assert.equal(await driver.expiredReceipt(record, intent, journal), null); assert.equal(stateReads, 1);
  });
});
test('proved expired absence is cached with audit metadata and immutable wire; same request never sends again', async () => {
  await fixture(async (driver, body, record, directory) => {
    const state = { nextNonce: '0', spentUnits: '0', mandate: record.mandate };
    const base = { ...driver.binding(record), state, decision: { allowed: true } };
    const intent = { owner: record.owner, policyId: record.policyId, requestId: body.requestId, requestHash: 'bound',
      inputHash: sha256(JSON.stringify(body)), transactionId: 'expired', base };
    const path = join(directory, 'requests', `${sha256(`${record.owner}\n${record.policyId}\n${body.requestId}`)}.json`);
    const journalPath = join(directory, 'transactions', 'expired.json');
    await durableJSON(path, intent); await durableJSON(journalPath, { signature: '1'.repeat(88), lastValidBlockHeight: '100', wire: 'immutable-signed-wire' });
    let reads = 0, validated = 0;
    driver.assertNetwork = async () => { validated++; };
    driver.fetchedState = async () => ({ state });
    driver.rpc = async (method, params) => {
      reads++;
      if (method === 'getBlockHeight') { assert.equal(params[0].commitment, 'finalized'); return 101; }
      if (method === 'getSignatureStatuses') return { value: [null] };
      assert.equal(method, 'getTransaction'); return null;
    };
    driver.submit = async () => { throw new Error('an expired ID must never be resubmitted'); };
    const [result] = await driver.requestReceipts(record, state);
    assert.equal(result.status, 'denied'); assert.equal(result.chainVerified, false);
    assert.equal(result.error.code, 'TRANSACTION_EXPIRED_NOT_LANDED'); assert.equal(result.metadata.outcome, 'expired_not_landed');
    assert.equal(result.metadata.expiryAudit.finalizedBlockHeight, '101'); assert.equal(result.metadata.expiryAudit.unchangedContractCounters, true);
    assert.equal(validated, 1); assert.equal((await readJSON(journalPath)).wire, 'immutable-signed-wire');
    assert.equal((await readJSON(journalPath)).status, 'expired_not_landed');
    assert.deepEqual(await driver.execute(body), result);
    const priorReads = reads; assert.deepEqual(await driver.requestReceipts(record, state), [result]); assert.equal(reads, priorReads);
  });
});
test('full mandate comparison rejects changed evidence authority before trusting head or token balances', async () => {
  const driver = new Driver({ programId: 'program' });
  driver.getAccount = async () => ({ owner: 'program', executable: false, data: ['fixture', 'base64'] });
  const mandate = { owner: 'owner', evidenceAuthority: { address: 'reviewed-attester' }, allocationUnits: '100' };
  driver.codec = async () => ({ mandate: { ...mandate, evidenceAuthority: { address: 'other-attester' } } });
  await assert.rejects(driver.fetchedState({ mandate, stateAddress: 'state' }), /full mandate differs/);
});
test('recovery requires the exact persisted signed wire, not nonce or spending resemblance', async () => {
  await fixture(async (driver, body, record, directory) => {
    const intent = { ...record, requestId: body.requestId, requestHash: 'bound', amountUnits: '1', status: 'submission_uncertain', transactionId: 'execute-fixture', base: { network: NETWORK } };
    await durableJSON(join(directory, 'requests', `${sha256('request')}.json`), intent);
    await durableJSON(join(directory, 'transactions', 'execute-fixture.json'), { signature: 'signature', wire: 'expected' });
    driver.rpc = async (method, params) => {
      if (method === 'getSignatureStatuses') return { value: [{ confirmationStatus: 'finalized' }] };
      if (method === 'getTransaction' && params[1].encoding === 'base64') return { transaction: ['different', 'base64'] };
      return { slot: 1, meta: { err: null } };
    };
    await assert.rejects(driver.requestReceipts(record, { nextNonce: '2', spentUnits: '2' }), /differs from persisted signed/);
  });
});
test('Kit round trip preserves existing state signature when owner finishes signing', async () => {
  const makeKey = async () => {
    const jwk = generateKeyPairSync('ed25519').privateKey.export({ format: 'jwk' });
    const bytes = Buffer.concat([Buffer.from(jwk.d, 'base64url'), Buffer.from(jwk.x, 'base64url')]);
    return { address: getAddressDecoder().decode(bytes.subarray(32)), keyPair: await createKeyPairFromBytes(bytes) };
  };
  const owner = await makeKey(); const state = await makeKey();
  const driver = new Driver({});
  driver.rpc = async () => ({ value: { blockhash: '11111111111111111111111111111111', lastValidBlockHeight: 123 } });
  const dto = { programAddress: '11111111111111111111111111111111', accounts: [{ address: owner.address, isSigner: true, isWritable: true }, { address: state.address, isSigner: true, isWritable: true }], dataBase64: '' };
  const prepared = await driver.build([dto], owner.address, [state]);
  const partial = getTransactionDecoder().decode(Buffer.from(prepared.transaction, 'base64'));
  assert.notEqual(partial.signatures[state.address], null); assert.equal(partial.signatures[owner.address], null);
  const full = await signTransaction([owner.keyPair], partial);
  const decoded = getTransactionDecoder().decode(Buffer.from(getBase64EncodedWireTransaction(full), 'base64'));
  assert.deepEqual(decoded.signatures[state.address], partial.signatures[state.address]);
  assert.notEqual(decoded.signatures[owner.address], null);
});
test('a submission makes one send RPC and permits only three validator forwards of its persisted signed wire', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'allowit-forwarding-'));
  try {
    const jwk = generateKeyPairSync('ed25519').privateKey.export({ format: 'jwk' });
    const bytes = Buffer.concat([Buffer.from(jwk.d, 'base64url'), Buffer.from(jwk.x, 'base64url')]);
    const executor = { address: getAddressDecoder().decode(bytes.subarray(32)), keyPair: await createKeyPairFromBytes(bytes) };
    const driver = new Driver({ privateDirectory: directory });
    const instruction = { programAddress: '11111111111111111111111111111111', accounts: [{ address: executor.address, isSigner: true, isWritable: true }], dataBase64: '' };
    let sends = 0, signedWire;
    driver.rpc = async (method, params) => {
      if (method === 'getLatestBlockhash') return { value: { blockhash: '11111111111111111111111111111111', lastValidBlockHeight: 123 } };
      if (method === 'sendTransaction') {
        sends++; assert.equal(params[1].maxRetries, 3); assert.equal(params[1].skipPreflight, false);
        const saved = await readJSON(join(directory, 'transactions', 'forwarding.json'));
        assert.equal(saved.status, 'signed_before_send'); assert.equal(params[0], saved.wire); signedWire = saved.wire;
        return saved.signature;
      }
      assert.equal(method, 'getTransaction'); return { transaction: [signedWire, 'base64'] };
    };
    driver.waitFinalized = async () => ({ receipt: { slot: 1, meta: { err: null } } });
    const result = await driver.submit([instruction], [executor], 'forwarding');
    assert.equal(typeof result.signature, 'string'); assert.equal(sends, 1);
    await assert.rejects(driver.submit([instruction], [executor], 'forwarding'), /EEXIST/); assert.equal(sends, 1);
  } finally { await rm(directory, { recursive: true, force: true }); }
});
test('a lost prepare response reuses the existing state key, activation ID and phase templates', async () => {
  await fixture(async (driver, body, record) => {
    const owner = '11111111111111111111111111111111';
    Object.assign(body, { owner, preset: 'numeric', source: 'exact source', originalIntent: 'exact intent', allocationUnits: '1' });
    Object.assign(record, { owner, preset: 'numeric', source: body.source, originalIntent: body.originalIntent, status: 'prepared', activationId: 'existing-activation', stateKeyPath: 'retained-private-key',
      phases: [{ label: 'phase' }], preparedTransactions: [{ label: 'phase', transaction: 'existing-wire' }], confirmedSignatures: [] });
    Object.assign(record.mandate, { owner, recipient: owner, action: 'invest', merchant: 'demo', allocationUnits: '1', revision: '1' });
    driver.config.recipient = owner;
    driver.config.presets = [{ id: 'numeric', source: body.source, originalIntent: body.originalIntent, allocationUnits: '1' }];
    await durableJSON(driver.recordPath(owner, body.policyId), record);
    driver.newStateKey = async () => { throw new Error('a retry must not create another state key'); };
    driver.prepared = async () => { throw new Error('existing complete phases must not be replaced'); };
    const recovered = await driver.activationPrepare(body);
    assert.equal(recovered.activationId, 'existing-activation'); assert.equal(recovered.stateAddress, 'state');
    assert.equal(recovered.transactions[0].transaction, 'existing-wire');
    await assert.rejects(driver.activationPrepare({ ...body, allocationUnits: '2' }), /differs from reviewed preset/);
  });
});
test('retrying revocation prepare preserves its original ID and unsigned owner message', async () => {
  await fixture(async (driver, body, record) => {
    record.revocationId = 'existing-revocation'; record.revocationTransaction = { transaction: 'same-wire', lastValidBlockHeight: '123' };
    driver.codec = async () => { throw new Error('revocation retry must not generate another instruction'); };
    driver.build = async () => { throw new Error('revocation retry must not replace the owner message'); };
    const result = await driver.revokePrepare(body);
    assert.equal(result.revocationId, 'existing-revocation'); assert.equal(result.transactions[0].transaction, 'same-wire');
  });
});
test('new and refreshed revoke wires include the authoritative heap budget and retain exact lifetime', async () => {
  await fixture(async (driver, body, record) => {
    const budget = (tag, value) => {
      const bytes = Buffer.alloc(5); bytes[0] = tag; bytes.writeUInt32LE(value, 1);
      return { programAddress: 'ComputeBudget111111111111111111111111111111', accounts: [], dataBase64: bytes.toString('base64') };
    };
    const computeBudgetInstructions = [budget(1, 256 * 1024), budget(2, 1_400_000)];
    const instruction = { programAddress: driver.config.programId, accounts: [], dataBase64: 'BA==' };
    let builds = 0;
    driver.codec = async input => {
      assert.equal(input.op, 'revoke'); assert.equal(input.owner, body.owner); assert.equal(input.stateAddress, record.stateAddress);
      return { instruction, computeBudgetInstructions };
    };
    driver.build = async (instructions, owner) => {
      assert.equal(owner, body.owner); assert.deepEqual(instructions, [...computeBudgetInstructions, instruction]);
      return { transaction: `owner-wire-${++builds}`, lastValidBlockHeight: String(123 + builds) };
    };
    const initial = await driver.revokePrepare(body);
    assert.equal(initial.transactions[0].transaction, 'owner-wire-1'); assert.equal(initial.transactions[0].lastValidBlockHeight, '124');
    const cached = await driver.revokePrepare(body);
    assert.equal(cached.revocationId, initial.revocationId); assert.equal(cached.transactions[0].transaction, 'owner-wire-1'); assert.equal(builds, 1);
    const refreshed = await driver.refreshTransaction({ ...body, revocationId: initial.revocationId });
    assert.equal(refreshed.transaction.transaction, 'owner-wire-2'); assert.equal(refreshed.transaction.lastValidBlockHeight, '125');
    assert.deepEqual(record.revocationTransaction, refreshed.transaction);
    record.pendingConfirmations = { revocation: 'already-broadcast-signature' };
    await assert.rejects(driver.refreshTransaction({ ...body, revocationId: initial.revocationId }), /signature is pending/);
    assert.equal(builds, 2);
  });
});
test('replaying a known confirmation never replaces the next phase message', async () => {
  await fixture(async (driver, body, record) => {
    Object.assign(record, { activationId: 'activation', phases: [{ label: 'first' }, { label: 'second' }],
      confirmedSignatures: ['first-signature'], preparedTransactions: [{ transaction: 'first-wire' }, { label: 'second', transaction: 'possibly-already-broadcast-wire' }] });
    driver.waitFinalized = async () => ({});
    driver.confirmedWire = async (signature, wire) => { assert.equal(signature, 'first-signature'); assert.equal(wire, 'first-wire'); };
    driver.prepared = async () => { throw new Error('a repeated confirmation must not replace an already-distributed wire'); };
    const result = await driver.confirm({ ...body, activationId: 'activation', transactionIndex: 0, signature: 'first-signature' });
    assert.equal(result.nextTransaction.transaction, 'possibly-already-broadcast-wire');
    assert.deepEqual(record.confirmedSignatures, ['first-signature']);
  });
});
function liveFixture(driver, body, record) {
  record.originalIntent = 'Reviewed exact intent';
  Object.assign(record.mandate, { allocationUnits: '500000000', action: 'invest', merchant: 'demo', network: NETWORK });
  body.context = { candidate_yield_bps: 800, benchmark_yield_bps: 850 };
  const question = 'Are the environmental claims independently supported?';
  const now = Math.floor(Date.now()/1000);
  body.semanticEvidence = { mode: 'live', question, key: sha256(question), scoreBps: '9000', providerReceipt: {
    provider: 'typesafe', model: 'jev-1.13.0', question, evidenceKey: sha256(question), scoreBps: '9000',
    assessmentContextHash: sha256('gateway-assessment-context'), observedAt: new Date().toISOString(), evaluatedAt: String(now),
    owner: record.owner, policyId: record.policyId, requestId: body.requestId, sourceHash: record.sourceHash, irHash: record.irHash,
    originalIntentHash: sha256(record.originalIntent), amountUnits: body.amountUnits, allocationUnits: '500000000', spentUnits: '0',
    action: 'invest', merchant: 'demo', recipient: record.mandate.recipient, token: 'USDC', network: NETWORK,
    runtimeContext: structuredClone(body.context), assessedNonce: '0',
  } };
  const assessmentStateJSON = JSON.stringify({ original_intent: record.originalIntent, runtime_context: body.context,
    request: { amount_units: Number(body.amountUnits), allocation_units: 500000000, spent_units: 0, action: 'invest', merchant: 'demo',
      recipient: record.mandate.recipient, token: 'USDC', network: NETWORK, now }, owner_answers: {} });
  body.semanticEvidence.providerReceipt.assessmentStateJSON = assessmentStateJSON;
  body.semanticEvidence.providerReceipt.assessmentContextHash = sha256(assessmentStateJSON);
  let preparedInput;
  driver.codec = async input => {
    if (input.op === 'semantic-key') return { key: sha256(input.question) };
    preparedInput = input;
    return { requestHash: 'bound-current-nonce', nonce: '0', contextHash: sha256(JSON.stringify(body.context)), runtimeContext: JSON.stringify(body.context),
      preflight: { allowed: false, status: 'denied', error: 'PolicyDenied' } };
  };
  driver.submit = async () => { throw new Error('fixture does not submit a transaction'); };
  return () => preparedInput;
}
test('live gateway assessments require an explicit boolean configuration switch and receipt', async () => {
  await fixture(async (driver, body, record) => {
    liveFixture(driver, body, record);
    await assert.rejects(driver.execute(body), /live gateway evidence is disabled/);
    driver.config.allowLiveEvidence = 'true';
    await assert.rejects(driver.execute(body), /live gateway evidence is disabled/);
    driver.config.allowLiveEvidence = true;
    delete body.semanticEvidence.providerReceipt;
    await assert.rejects(driver.execute(body), /requires the trusted gateway provider receipt/);
  });
});
test('live receipt matches current nonce, context and immutable mandate while retaining its real mode', async () => {
  await fixture(async (driver, body, record) => {
    driver.config.allowLiveEvidence = true;
    const captured = liveFixture(driver, body, record);
    const result = await driver.execute(body);
    assert.equal(result.semanticMode, 'live'); assert.equal(result.assessmentSource, 'trusted_go_gateway');
    assert.equal(result.providerReceipt.provider, 'typesafe'); assert.equal(result.providerReceipt.assessedNonce, '0');
    assert.equal(captured().evidence.intervals[0].name, sha256(body.semanticEvidence.question));
    assert.equal(captured().evidence.intervals[0].lowerBps, '9000'); assert.equal(captured().nonce, '0');
    const stale = structuredClone(body); stale.requestId = 'stale-assessment'; stale.semanticEvidence.providerReceipt.requestId = stale.requestId;
    stale.semanticEvidence.providerReceipt.assessedNonce = '1';
    await assert.rejects(driver.execute(stale), /mismatched assessedNonce/);
    const changed = structuredClone(body); changed.requestId = 'changed-context'; changed.semanticEvidence.providerReceipt.requestId = changed.requestId;
    changed.context.candidate_yield_bps = 100;
    await assert.rejects(driver.execute(changed), /runtime context differs/);
    const budget = structuredClone(body); budget.requestId = 'changed-allocation'; budget.semanticEvidence.providerReceipt.requestId = budget.requestId;
    budget.semanticEvidence.providerReceipt.allocationUnits = '999999999';
    await assert.rejects(driver.execute(budget), /mismatched allocationUnits/);
    const bytes = structuredClone(body); bytes.requestId = 'tampered-assessment-state'; bytes.semanticEvidence.providerReceipt.requestId = bytes.requestId;
    bytes.semanticEvidence.providerReceipt.assessmentStateJSON += ' ';
    await assert.rejects(driver.execute(bytes), /assessment state hash differs/);
  });
});
test('public semantic capabilities reflect explicit configuration without making provider calls', async () => {
  await fixture(async driver => {
    assert.deepEqual((await driver.info()).semanticModes, ['mock_explicit']);
    driver.config.allowLiveEvidence = true;
    const info = await driver.info(); assert.deepEqual(info.semanticModes, ['live', 'mock_explicit']);
    assert.equal(info.assessmentSource, 'trusted_go_gateway');
  });
});
test('unsafe or fractional runtime context is rejected before compiler or assessment processing', async () => {
  await fixture(async (driver, body) => {
    driver.codec = async () => { throw new Error('invalid context must not reach the codec'); };
    for (const number of [9007199254740992, 0.1]) await assert.rejects(driver.execute({ ...body, context: { nested: [number] } }), /exact safe integers/);
  });
});
