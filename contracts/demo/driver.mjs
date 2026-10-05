import { spawn } from 'node:child_process';
import { generateKeyPairSync, randomUUID } from 'node:crypto';
import { mkdir, realpath, readdir } from 'node:fs/promises';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  address, blockhash, pipe, createTransactionMessage, setTransactionMessageFeePayer,
  setTransactionMessageLifetimeUsingBlockhash, appendTransactionMessageInstructions,
  compileTransaction, partiallySignTransaction, signTransaction, createKeyPairFromBytes,
  getBase64EncodedWireTransaction, getSignatureFromTransaction, getTransactionDecoder,
  getAddressEncoder, getAddressDecoder, getProgramDerivedAddress,
  compressTransactionMessageUsingAddressLookupTables,
} from '@solana/kit';
import { NETWORK, GENESIS_HASH, MINT, TOKEN_PROGRAM, sha256, decimal, outsideRepository,
  durableJSON, readJSON, policyStorageKey, tokenAccountFields, validateContextNumbers, SerialQueue } from './safety.mjs';
import { callSolanaRpc } from './rpc.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPOSITORY = resolve(HERE, '../..');
const SYSTEM = '11111111111111111111111111111111';
const ASSOCIATED = 'ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL';
const LOOKUP = 'AddressLookupTab1e1111111111111111111111111';
const UPGRADEABLE_LOADER = 'BPFLoaderUpgradeab1e11111111111111111111111';
const RENT = 'SysvarRent111111111111111111111111111111111';
const CLOCK = 'SysvarC1ock11111111111111111111111111111111';
const encodeAddress = value => Buffer.from(getAddressEncoder().encode(address(value)));
const decodeAddress = bytes => getAddressDecoder().decode(bytes);
const account = (value, signer = false, writable = false) => ({ address: value, isSigner: signer, isWritable: writable });
const instruction = (program, accounts, bytes) => ({ programAddress: program, accounts, dataBase64: Buffer.from(bytes).toString('base64') });
const u32 = value => { const data = Buffer.alloc(4); data.writeUInt32LE(value); return data; };
const u64 = value => { const data = Buffer.alloc(8); data.writeBigUInt64LE(BigInt(value)); return data; };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const canonical = value => Array.isArray(value) ? `[${value.map(canonical).join(',')}]` :
  value !== null && typeof value === 'object' ? `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}` : JSON.stringify(value);

export class Driver {
  constructor(config) { this.config = config; this.queue = new SerialQueue(); this.rpcId = 0; }
  static async load(path) {
    outsideRepository(await realpath(path), await realpath(dirname(REPOSITORY)));
    const config = await readJSON(path);
    const privateDirectory = outsideRepository(config.privateDirectory, dirname(REPOSITORY));
    await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
    config.privateDirectory = outsideRepository(await realpath(privateDirectory), await realpath(dirname(REPOSITORY)));
    if (config.genesisHash !== GENESIS_HASH || config.network !== NETWORK || (config.mint && config.mint !== MINT)) throw new Error('config must explicitly bind canonical Solana Devnet');
    if (!config.bearerToken || config.bearerToken.length < 24) throw new Error('sidecar bearer token must contain at least 24 characters');
    config.codecPath = resolve(config.codecPath);
    address(config.programId); address(config.recipient);
    const driver = new Driver(config); driver.keys = {};
    for (const role of ['executor', 'compiler', 'evidence']) {
      const keyPath = outsideRepository(resolve(config.keys[role]), dirname(REPOSITORY));
      outsideRepository(await realpath(keyPath), await realpath(dirname(REPOSITORY)));
      driver.keys[role] = await driver.keypair(keyPath);
    }
    await driver.assertNetwork();
    return driver;
  }
  async keypair(path) {
    const bytes = Uint8Array.from(await readJSON(path));
    if (bytes.length !== 64) throw new Error('expected a 64-byte Solana keypair file');
    return { path, address: decodeAddress(bytes.subarray(32)), keyPair: await createKeyPairFromBytes(bytes) };
  }
  async newStateKey(id) {
    const { privateKey } = generateKeyPairSync('ed25519');
    const jwk = privateKey.export({ format: 'jwk' });
    const bytes = Buffer.concat([Buffer.from(jwk.d, 'base64url'), Buffer.from(jwk.x, 'base64url')]);
    const path = join(this.config.privateDirectory, 'states', `${id}.json`);
    await durableJSON(path, [...bytes], true);
    return this.keypair(path);
  }
  async rpc(method, params = []) {
    return callSolanaRpc(this.config.rpcUrl, { jsonrpc: '2.0', id: ++this.rpcId, method, params }, { minimumIntervalMs: 500 });
  }
  async assertNetwork() {
    const genesis = await this.rpc('getGenesisHash');
    if (genesis !== GENESIS_HASH) throw new Error(`unexpected RPC genesis: ${genesis}`);
    const program = await this.getAccount(this.config.programId);
    if (!program?.executable || program.owner !== UPGRADEABLE_LOADER) throw new Error('configured program is not deployed under the pinned upgradeable loader');
    const programBytes = Buffer.from(program.data[0], 'base64');
    if (programBytes.length !== 36 || programBytes.readUInt32LE(0) !== 2) throw new Error('invalid upgradeable program account');
    const programDataAddress = decodeAddress(programBytes.subarray(4, 36));
    const programData = await this.getAccount(programDataAddress);
    if (!programData || programData.owner !== UPGRADEABLE_LOADER || programData.executable) throw new Error('invalid program-data ownership');
    const data = Buffer.from(programData.data[0], 'base64');
    if (data.length < 45 || data.readUInt32LE(0) !== 3 || ![0,1].includes(data[12])) throw new Error('invalid program-data header');
    const authority = data[12] ? decodeAddress(data.subarray(13, 45)) : null;
    if (this.config.expectedUpgradeAuthority === undefined || authority !== this.config.expectedUpgradeAuthority) throw new Error('program upgrade authority differs from deployment manifest');
    const length = Number(decimal(String(this.config.expectedProgramBytes), 'expectedProgramBytes'));
    if (!Number.isSafeInteger(length) || length <= 0 || length > data.length - 45) throw new Error('invalid pinned ELF length');
    if (!/^[0-9a-f]{64}$/.test(this.config.expectedProgramSha256 ?? '') || sha256(data.subarray(45,45+length)) !== this.config.expectedProgramSha256) throw new Error('deployed ELF hash differs from reviewed build');
    if (data.subarray(45+length).some(byte => byte !== 0)) throw new Error('unexpected nonzero program-data padding');
    const mint = await this.getAccount(MINT);
    if (!mint || mint.owner !== TOKEN_PROGRAM || mint.executable) throw new Error('canonical USDC mint has wrong token-program owner');
    const mintBytes = Buffer.from(mint.data[0], 'base64');
    if (mintBytes.length !== 82 || mintBytes[44] !== 6 || mintBytes[45] !== 1) throw new Error('canonical USDC mint decimals or initialization differ');
  }
  codec(input) {
    return new Promise((resolveResult, reject) => {
      const payload = JSON.stringify(input);
      if (Buffer.byteLength(payload) > 128 * 1024) return reject(new Error('codec input exceeds 128KiB'));
      const child = spawn(this.config.codecPath, [], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
      const timeout = setTimeout(() => { child.kill(); reject(new Error('codec process exceeded 30 seconds')); }, 30_000);
      const stdout = []; const stderr = []; let size = 0;
      child.stdout.on('data', data => { size += data.length; if (size > 2_000_000) child.kill(); else stdout.push(data); });
      child.stderr.on('data', data => { if (stderr.length < 8) stderr.push(data); });
      child.on('error', error => { clearTimeout(timeout); reject(error); });
      child.stdin.on('error', error => { clearTimeout(timeout); reject(error); });
      child.on('close', code => {
        clearTimeout(timeout);
        if (code !== 0) return reject(new Error(`codec failed (${code}): ${Buffer.concat(stderr).toString().slice(0, 2000)}`));
        try { const output = JSON.parse(Buffer.concat(stdout).toString()); if (!output.ok) throw new Error(output.error); resolveResult(output); } catch (error) { reject(error); }
      });
      child.stdin.end(payload);
    });
  }
  async getAccount(key, commitment = 'finalized') {
    return (await this.rpc('getAccountInfo', [key, { commitment, encoding: 'base64' }])).value;
  }
  recordPath(owner, policyId) { return join(this.config.privateDirectory, 'policies', `${policyStorageKey(owner, policyId)}.json`); }
  async record(owner, policyId) {
    const record = await readJSON(this.recordPath(owner, policyId), null);
    if (!record || record.owner !== owner || record.policyId !== policyId) throw new Error('unknown owner policy activation');
    return record;
  }
  binding(record) { return { owner: record.owner, policyId: record.policyId, sourceHash: record.sourceHash, irHash: record.irHash,
    network: NETWORK, genesisHash: GENESIS_HASH, programId: this.config.programId, mint: MINT }; }
  async associated(owner) {
    const [key] = await getProgramDerivedAddress({ programAddress: address(ASSOCIATED), seeds: [encodeAddress(owner), encodeAddress(TOKEN_PROGRAM), encodeAddress(MINT)] });
    return key;
  }
  createAssociated(payer, owner, associated) {
    return instruction(ASSOCIATED, [account(payer, true, true), account(associated, false, true), account(owner), account(MINT), account(SYSTEM), account(TOKEN_PROGRAM)], [1]);
  }
  async token(key, expectedOwner) {
    const raw = await this.getAccount(key);
    if (!raw || raw.owner !== TOKEN_PROGRAM || raw.executable) throw new Error('canonical classic USDC token account is missing');
    const fields = tokenAccountFields(raw.data[0]);
    if (!fields.mint.equals(encodeAddress(MINT)) || !fields.owner.equals(encodeAddress(expectedOwner)) || fields.state !== 1) throw new Error('token mint, authority or state does not match mandate');
    return fields;
  }
  async lookupAddresses(key) {
    const raw = await this.getAccount(key);
    if (!raw || raw.owner !== LOOKUP || raw.executable) throw new Error('invalid lookup table owner');
    const data = Buffer.from(raw.data[0], 'base64');
    if (data.length < 56 || (data.length - 56) % 32 || data.readUInt32LE(0) !== 1 || data.readBigUInt64LE(4) !== (1n << 64n) - 1n) throw new Error('invalid or deactivated lookup table');
    const slot = BigInt(await this.rpc('getSlot', [{ commitment: 'finalized' }]));
    if (data.readBigUInt64LE(12) >= slot) throw new Error('lookup table extension is not usable yet');
    return Array.from({ length: (data.length - 56) / 32 }, (_, i) => decodeAddress(data.subarray(56 + i * 32, 88 + i * 32)));
  }
  kitInstruction(dto) {
    return { programAddress: address(dto.programAddress), data: Uint8Array.from(Buffer.from(dto.dataBase64, 'base64')),
      accounts: dto.accounts.map(a => ({ address: address(a.address), role: (a.isSigner ? 2 : 0) + (a.isWritable ? 1 : 0) })) };
  }
  async build(instructions, feePayer, keys = [], lookupAddress) {
    const lifetime = (await this.rpc('getLatestBlockhash', [{ commitment: 'confirmed' }])).value;
    const make = version => pipe(createTransactionMessage({ version }),
      message => setTransactionMessageFeePayer(address(feePayer), message),
      message => setTransactionMessageLifetimeUsingBlockhash({ blockhash: blockhash(lifetime.blockhash), lastValidBlockHeight: BigInt(lifetime.lastValidBlockHeight) }, message),
      message => appendTransactionMessageInstructions(instructions.map(dto => this.kitInstruction(dto)), message));
    let message = make('legacy'); let transaction = compileTransaction(message);
    let wire = getBase64EncodedWireTransaction(transaction);
    if (Buffer.from(wire, 'base64').length > 1232 && lookupAddress) {
      message = compressTransactionMessageUsingAddressLookupTables(make(0), { [lookupAddress]: (await this.lookupAddresses(lookupAddress)).map(address) });
      transaction = compileTransaction(message); wire = getBase64EncodedWireTransaction(transaction);
    }
    if (Buffer.from(wire, 'base64').length > 1232) throw new Error(`transaction_packet_limit:${Buffer.from(wire, 'base64').length}`);
    if (keys.length) transaction = await partiallySignTransaction(keys.map(key => key.keyPair), transaction);
    return { transaction: getBase64EncodedWireTransaction(transaction), lastValidBlockHeight: String(lifetime.lastValidBlockHeight) };
  }
  async waitFinalized(signature, lastValidBlockHeight) {
    const deadline = Date.now() + (this.config.finalityTimeoutMs ?? 60_000);
    while (Date.now() < deadline) {
      const status = (await this.rpc('getSignatureStatuses', [[signature], { searchTransactionHistory: true }])).value[0];
      if (status?.confirmationStatus === 'finalized') {
        const receipt = await this.rpc('getTransaction', [signature, { commitment: 'finalized', encoding: 'json', maxSupportedTransactionVersion: 0 }]);
        if (!receipt) throw new Error('finalized transaction receipt unavailable');
        return { status, receipt };
      }
      // A landed transaction may still finalize after blockhash expiry.
      if (!status && lastValidBlockHeight && BigInt(await this.rpc('getBlockHeight', [{ commitment: 'confirmed' }])) > BigInt(lastValidBlockHeight)) throw new Error(`transaction_expired:${signature}`);
      await sleep(1500);
    }
    throw new Error(`submission_pending_query_signature:${signature}`);
  }
  async submit(instructions, keys, id, lookupAddress, negative = false) {
    const built = await this.build(instructions, keys[0].address, [], lookupAddress);
    const signed = await signTransaction(keys.map(key => key.keyPair), getTransactionDecoder().decode(Buffer.from(built.transaction, 'base64')));
    const wire = getBase64EncodedWireTransaction(signed); const signature = getSignatureFromTransaction(signed);
    const path = join(this.config.privateDirectory, 'transactions', `${id}.json`);
    const journal = { id, signature, wire, lastValidBlockHeight: built.lastValidBlockHeight, status: 'signed_before_send', createdAt: new Date().toISOString() };
    await durableJSON(path, journal, true);
    // One HTTP submission; the validator may forward this identical signed wire
    // up to three times. Never replace its blockhash or resubmit an uncertain ID.
    let transport;
    try { transport = await this.rpc('sendTransaction', [wire, { encoding: 'base64', skipPreflight: negative, preflightCommitment: 'confirmed', maxRetries: 3 }]); }
    catch (error) { journal.status = 'submission_uncertain'; journal.error = error.message; await durableJSON(path, journal); throw new Error(`${error.message}; inspect persisted signature ${signature}`); }
    if (transport !== signature) throw new Error('RPC returned a different transaction signature');
    journal.status = 'submitted'; await durableJSON(path, journal);
    const result = await this.waitFinalized(signature, built.lastValidBlockHeight);
    const finalizedWire = await this.rpc('getTransaction', [signature, { commitment: 'finalized', encoding: 'base64', maxSupportedTransactionVersion: 0 }]);
    if (!finalizedWire || finalizedWire.transaction[0] !== wire) throw new Error('finalized receipt differs from persisted signed executor wire');
    journal.status = result.receipt.meta?.err ? 'finalized_failed' : 'finalized'; journal.receipt = result.receipt;
    await durableJSON(path, journal);
    return { signature, receipt: result.receipt };
  }
  async ensureLookup(record) {
    if (record.lookupAddress) return record.lookupAddress;
    if (this.config.lookupAddress) { await this.lookupAddresses(this.config.lookupAddress); record.lookupAddress = this.config.lookupAddress; return record.lookupAddress; }
    const authority = this.keys.executor.address;
    const slot = BigInt(await this.rpc('getSlot', [{ commitment: 'finalized' }]));
    const [table, bump] = await getProgramDerivedAddress({ programAddress: address(LOOKUP), seeds: [encodeAddress(authority), u64(slot)] });
    const create = instruction(LOOKUP, [account(table, false, true), account(authority, true), account(authority, true, true), account(SYSTEM)], Buffer.concat([u32(0), u64(slot), Buffer.from([bump])]));
    await this.submit([create], [this.keys.executor], `lookup-create-${randomUUID()}`);
    const addresses = [...new Set([record.stateAddress, record.headAddress, record.delegateAddress, record.sourceTokenAccount, record.destinationTokenAccount, MINT, TOKEN_PROGRAM, CLOCK, RENT, SYSTEM, this.config.programId])];
    const extend = instruction(LOOKUP, [account(table, false, true), account(authority, true), account(authority, true, true), account(SYSTEM)], Buffer.concat([u32(2), u64(addresses.length), ...addresses.map(encodeAddress)]));
    await this.submit([extend], [this.keys.executor], `lookup-extend-${randomUUID()}`);
    record.lookupAddress = table;
    await durableJSON(this.recordPath(record.owner, record.policyId), record);
    await this.lookupAddresses(table);
    return table;
  }
  async prepared(record, phase) {
    const keys = [];
    if (phase.stateSigner) keys.push(await this.keypair(record.stateKeyPath));
    if (phase.compilerSigner) keys.push(this.keys.compiler);
    let built;
    try { built = await this.build(phase.instructions, record.owner, keys, record.lookupAddress); }
    catch (error) { if (!error.message.startsWith('transaction_packet_limit:')) throw error;
      built = await this.build(phase.instructions, record.owner, keys, await this.ensureLookup(record)); }
    return { label: phase.label, ...built };
  }
  async info() {
    await this.assertNetwork();
    return { network: NETWORK, genesisHash: GENESIS_HASH, programId: this.config.programId, mint: MINT, recipient: this.config.recipient,
      executor: this.keys.executor.address, compiler: this.keys.compiler.address,
      evidenceAuthority: { address: this.keys.evidence.address, keyId: 'demo-evidence', version: '0.1.0' },
      semanticModes: this.config.allowLiveEvidence === true ? ['live', 'mock_explicit'] : ['mock_explicit'],
      assessmentSource: 'trusted_go_gateway', walletReviewSupported: false, presets: this.config.presets ?? [],
      limits: { artifactBytes: 8192, contextBytes: 256, evidenceLifetimeSeconds: 300, instructionBytes: 1024 } };
  }
  activationResponse(record) {
    return { ...this.binding(record), activationId: record.activationId, source: record.source, originalIntent: record.originalIntent,
      compiledIR: record.compiledIR, recipient: record.mandate.recipient, budgetUnits: record.mandate.allocationUnits,
      stateAddress: record.stateAddress, headAddress: record.headAddress, delegateAddress: record.delegateAddress,
      revision: record.mandate.revision, expiresAt: record.mandate.expiresAt, compiler: record.mandate.compiler,
      executor: record.mandate.executor, evidenceAuthority: record.mandate.evidenceAuthority, mandate: record.mandate,
      transactions: record.preparedTransactions, confirmedSignatures: record.confirmedSignatures, nextIndex: record.confirmedSignatures.length };
  }
  async activationPrepare(body) {
    address(body.owner);
    if (!/^[0-9a-f]{64}$/.test(body.policyId ?? '')) throw new Error('policyId must be 32-byte lowercase hex');
    return this.queue.run(policyStorageKey(body.owner, body.policyId), async () => {
      await this.assertNetwork();
      const preset = (this.config.presets ?? []).find(preset => preset.id === body.preset);
      if (!preset) throw new Error('choose a configured reviewed preset');
      const source = body.source ?? preset?.source;
      const originalIntent = body.originalIntent ?? preset?.originalIntent;
      if (typeof source !== 'string' || typeof originalIntent !== 'string') throw new Error('exact source and originalIntent are required');
      const allocationUnits = body.allocationUnits ?? preset?.allocationUnits; decimal(allocationUnits, 'allocationUnits');
      if (source !== preset.source || originalIntent !== preset.originalIntent || allocationUnits !== preset.allocationUnits) throw new Error('source, intent or allocation differs from reviewed preset');
      const action = preset.action ?? 'invest'; const merchant = preset.merchant ?? 'demo';
      if ((body.action && body.action !== action) || (body.merchant && body.merchant !== merchant)) throw new Error('action or merchant differs from reviewed preset');
      const recipient = body.recipient ?? this.config.recipient; address(recipient);
      if (recipient !== this.config.recipient) throw new Error('recipient differs from configured reviewed destination');
      const existing = await readJSON(this.recordPath(body.owner, body.policyId), null);
      if (existing) {
        if (existing.source !== source || existing.originalIntent !== originalIntent || existing.mandate.allocationUnits !== allocationUnits || existing.mandate.recipient !== recipient || existing.mandate.action !== action || existing.mandate.merchant !== merchant || existing.preset !== body.preset || (body.expiresAt && body.expiresAt !== existing.mandate.expiresAt)) throw new Error('policyId already names different immutable activation details');
        if (existing.status !== 'prepared') throw new Error('policyId already has an active or revoked mandate; choose a new policyId');
        // A lost prepare response never replaces the state key, head or compiler
        // authorization. Complete only templates not yet persisted by a crash.
        for (let i=existing.preparedTransactions.length;i<existing.phases.length;i++) existing.preparedTransactions.push(await this.prepared(existing, existing.phases[i]));
        await durableJSON(this.recordPath(existing.owner, existing.policyId), existing);
        return this.activationResponse(existing);
      }
      const activationId = randomUUID(); const stateKey = await this.newStateKey(activationId);
      const addresses = await this.codec({ op: 'addresses', programId: this.config.programId, owner: body.owner, policyId: body.policyId, stateAddress: stateKey.address });
      const head = await this.getAccount(addresses.headAddress);
      let revision = 1n;
      if (head) {
        const raw = Buffer.from(head.data[0], 'base64');
        if (head.owner !== this.config.programId || raw.length !== 48 || raw.subarray(0, 8).toString() !== 'ALTHD001') throw new Error('invalid policy head');
        revision = raw.readBigUInt64LE(8) + 1n;
      }
      const expiresAt = body.expiresAt ?? String(Math.floor(Date.now() / 1000) + 86400);
      if (decimal(expiresAt, 'expiresAt') <= BigInt(Math.floor(Date.now()/1000) + 300)) throw new Error('mandate expiry is too soon for wallet activation');
      const prepared = await this.codec({ op: 'activation', programId: this.config.programId, owner: body.owner, policyId: body.policyId,
        stateAddress: stateKey.address, source, originalIntent, executor: this.keys.executor.address, compiler: this.keys.compiler.address,
        evidenceAuthority: { address: this.keys.evidence.address, keyId: 'demo-evidence', version: '0.1.0' }, recipient,
        revision: revision.toString(), expiresAt, allocationUnits, action, merchant, uploadChunkBytes: 480 });
      const sourceTokenAccount = await this.associated(body.owner); const destinationTokenAccount = await this.associated(recipient);
      const sourceToken = await this.token(sourceTokenAccount, body.owner);
      if (sourceToken.amount === 0n) throw new Error(`owner has no canonical Devnet USDC; fund ${sourceTokenAccount}`);
      const rent = String(await this.rpc('getMinimumBalanceForRentExemption', [prepared.stateBytes, { commitment: 'finalized' }]));
      const createState = await this.codec({ op: 'create-state', owner: body.owner, stateAddress: stateKey.address, programId: this.config.programId, lamports: rent });
      const approval = await this.codec({ op: 'approve', owner: body.owner, sourceTokenAccount, delegateAddress: prepared.delegateAddress, allocationUnits });
      const phases = [];
      const budget = prepared.computeBudgetInstructions;
      if (!head) phases.push({ label: 'Create policy head', instructions: [...budget, prepared.instructions[0]] });
      phases.push({ label: 'Create mandate account', instructions: [createState.instruction], stateSigner: true });
      for (const dto of prepared.instructions.slice(1)) phases.push({ label: dto.label, instructions: [...budget, dto], stateSigner: dto.label === 'initialize-state', compilerSigner: dto.label === 'activate' });
      const destination = await this.getAccount(destinationTokenAccount);
      phases.push({ label: 'Approve contract delegate and destination account', instructions: [
        ...(!destination ? [this.createAssociated(body.owner, recipient, destinationTokenAccount)] : []), approval.instruction] });
      const record = { activationId, owner: body.owner, policyId: body.policyId, source, originalIntent, preset: body.preset,
        sourceHash: prepared.metadata.sourceHash, irHash: prepared.metadata.irHash, compiledIR: prepared.metadata.compiledIR,
        artifactHash: prepared.metadata.artifactHash, mandateHash: prepared.mandateHash, mandate: prepared.mandate,
        stateAddress: stateKey.address, stateKeyPath: stateKey.path, headAddress: prepared.headAddress, delegateAddress: prepared.delegateAddress,
        sourceTokenAccount, destinationTokenAccount, phases, preparedTransactions: [], confirmedSignatures: [], status: 'prepared' };
      await durableJSON(this.recordPath(body.owner, body.policyId), record);
      // Every following phase is refreshed after confirming its predecessor.
      for (const phase of phases) record.preparedTransactions.push(await this.prepared(record, phase));
      await durableJSON(this.recordPath(body.owner, body.policyId), record);
      return this.activationResponse(record);
    });
  }
  async confirmedWire(signature, expectedWire) {
    const tx = await this.rpc('getTransaction', [signature, { commitment: 'finalized', encoding: 'base64', maxSupportedTransactionVersion: 0 }]);
    if (!tx || tx.meta?.err) throw new Error(`transaction failed or not finalized:${signature}`);
    const actual = getTransactionDecoder().decode(Buffer.from(tx.transaction[0], 'base64'));
    const expected = getTransactionDecoder().decode(Buffer.from(expectedWire, 'base64'));
    if (!Buffer.from(actual.messageBytes).equals(Buffer.from(expected.messageBytes))) throw new Error('wallet transaction differs from prepared owner-approved message');
    await durableJSON(join(this.config.privateDirectory, 'wallet-receipts', `${signature}.json`), { signature, wire: tx.transaction[0], receipt: tx });
    return tx;
  }
  async confirm(body) {
    return this.queue.run(policyStorageKey(body.owner, body.policyId), async () => {
      const record = await this.record(body.owner, body.policyId);
      const pendingKey = body.activationId ? `activation:${body.transactionIndex}` : 'revocation';
      if (body.activationId && (body.activationId !== record.activationId || !Number.isInteger(body.transactionIndex) || body.transactionIndex < 0 || body.transactionIndex >= record.phases.length)) throw new Error('invalid activation phase');
      if (!body.activationId && (!body.revocationId || body.revocationId !== record.revocationId)) throw new Error('wrong revocation ID');
      record.pendingConfirmations ??= {};
      if (record.pendingConfirmations[pendingKey] && record.pendingConfirmations[pendingKey] !== body.signature) throw new Error('a different signature is already pending for this owner phase');
      record.pendingConfirmations[pendingKey] = body.signature;
      await durableJSON(this.recordPath(record.owner, record.policyId), record);
      await this.waitFinalized(body.signature, body.lastValidBlockHeight);
      let nextTransaction;
      if (body.activationId) {
        if (body.activationId !== record.activationId || !Number.isInteger(body.transactionIndex) || body.transactionIndex < 0 || body.transactionIndex >= record.phases.length) throw new Error('invalid activation phase');
        const index = body.transactionIndex;
        const alreadyConfirmed = record.confirmedSignatures[index];
        if (alreadyConfirmed && alreadyConfirmed !== body.signature) throw new Error('activation phase already has a different signature');
        if (!alreadyConfirmed && record.confirmedSignatures.length !== index) throw new Error('activation phases must be confirmed in order');
        await this.confirmedWire(body.signature, record.preparedTransactions[index].transaction);
        if (!alreadyConfirmed) record.confirmedSignatures.push(body.signature);
        delete record.pendingConfirmations[pendingKey];
        if (!alreadyConfirmed && index + 1 < record.phases.length && index + 1 === record.confirmedSignatures.length && !record.pendingConfirmations[`activation:${index+1}`]) {
          nextTransaction = await this.prepared(record, record.phases[index+1]);
          record.preparedTransactions[index+1] = nextTransaction;
        } else if (index + 1 < record.phases.length) {
          nextTransaction = record.preparedTransactions[index+1];
        }
        await durableJSON(this.recordPath(record.owner, record.policyId), record);
      } else if (body.revocationId) {
        if (body.revocationId !== record.revocationId) throw new Error('wrong revocation ID');
        await this.confirmedWire(body.signature, record.revocationTransaction.transaction);
        record.revocationSignature = body.signature;
        delete record.pendingConfirmations[pendingKey];
        await durableJSON(this.recordPath(record.owner, record.policyId), record);
      } else {
        throw new Error('confirmation requires a prepared activationId or revocationId');
      }
      return { ...this.binding(record), status: 'finalized', chainVerified: true, signature: body.signature, nextTransaction };
    });
  }
  async refreshTransaction(body) {
    return this.queue.run(policyStorageKey(body.owner, body.policyId), async () => {
      await this.assertNetwork();
      const record = await this.record(body.owner, body.policyId);
      let transaction;
      if (body.activationId) {
        if (body.activationId !== record.activationId || body.transactionIndex !== record.confirmedSignatures.length || body.transactionIndex >= record.phases.length) throw new Error('only the next unconfirmed activation phase can be refreshed');
        if (record.pendingConfirmations?.[`activation:${body.transactionIndex}`]) throw new Error('owner transaction signature is pending; confirm that exact signature before refreshing');
        transaction = await this.prepared(record, record.phases[body.transactionIndex]);
        record.preparedTransactions[body.transactionIndex] = transaction;
      } else if (body.revocationId === record.revocationId && body.revocationId) {
        if (record.revocationSignature) throw new Error('revocation already has a signature; confirm it before refreshing');
        if (record.pendingConfirmations?.revocation) throw new Error('revocation signature is pending; confirm it before refreshing');
        const dto = await this.codec({ op: 'revoke', programId: this.config.programId, owner: body.owner, stateAddress: record.stateAddress });
        transaction = { label: 'Permanently revoke this mandate', ...await this.build([...dto.computeBudgetInstructions, dto.instruction], body.owner) };
        record.revocationTransaction = transaction;
      } else { throw new Error('refresh requires the existing activation or revocation ID'); }
      await durableJSON(this.recordPath(record.owner, record.policyId), record);
      return { ...this.binding(record), transaction, nextTransaction: transaction };
    });
  }
  async fetchedState(record) {
    const raw = await this.getAccount(record.stateAddress);
    if (!raw || raw.owner !== this.config.programId || raw.executable) throw new Error('state account is missing or has wrong program owner');
    const state = await this.codec({ op: 'decode-state', stateBase64: raw.data[0] });
    const m = state.mandate;
    // The owner/compiler approved every mandate field, including the evidence
    // authority, exact allocation, expiry and action. Compare the full binding.
    if (JSON.stringify(m) !== JSON.stringify(record.mandate)) throw new Error('finalized full mandate differs from owner-reviewed activation');
    for (const [actual, expected] of [[m.owner, record.owner], [m.policyId, record.policyId], [m.sourceHash, record.sourceHash], [m.irHash, record.irHash], [m.artifactHash, record.artifactHash], [m.executor, this.keys.executor.address], [m.compiler, this.keys.compiler.address], [m.recipient, record.mandate.recipient], [m.network, NETWORK], [m.asset, MINT]]) {
      if (actual !== expected) throw new Error('finalized mandate differs from owner-reviewed activation');
    }
    const head = await this.getAccount(record.headAddress);
    if (!head || head.owner !== this.config.programId) throw new Error('missing program-owned head');
    const headData = Buffer.from(head.data[0], 'base64');
    if (headData.length !== 48 || headData.subarray(0,8).toString() !== 'ALTHD001' || headData.readBigUInt64LE(8).toString() !== m.revision || decodeAddress(headData.subarray(16,48)) !== record.stateAddress) throw new Error('head does not select this mandate revision');
    Object.assign(state, { allocationUnits: m.allocationUnits, expiresAt: m.expiresAt, revision: m.revision,
      status: state.revoked ? 'revoked' : state.active ? 'active' : 'inactive' });
    return { state, stateBase64: raw.data[0] };
  }
  unknownReceipt(record, intent, journal, reason) {
    journal ??= intent.signature ? { signature: intent.signature } : null;
    return { ...this.binding(record), ...intent.base, requestId: intent.requestId, requestHash: intent.requestHash,
      status: 'unknown', chainVerified: false, ...(journal?.signature ? {
        signature: journal.signature, explorerUrl: `https://explorer.solana.com/tx/${journal.signature}?cluster=devnet`,
        lastValidBlockHeight: journal.lastValidBlockHeight,
      } : {}), error: reason ?? intent.error ?? journal?.error ?? 'The signed execution has no verified terminal outcome. Query this same request; do not submit a replacement.' };
  }
  async expiredReceipt(record, intent, journal) {
    if (!journal?.signature || journal.lastValidBlockHeight === undefined || !intent.base?.state) return null;
    const expiry = decimal(String(journal.lastValidBlockHeight), 'lastValidBlockHeight');
    const height = await this.rpc('getBlockHeight', [{ commitment: 'finalized' }]);
    if (!Number.isSafeInteger(height) || height < 0) throw new Error('invalid finalized block height');
    if (BigInt(height) <= expiry) return null;
    // Confirmed-height expiry cannot prove absence: a landed transaction may
    // still finalize. Recheck history only after a finalized height beyond expiry.
    const history = (await this.rpc('getSignatureStatuses', [[journal.signature], { searchTransactionHistory: true }])).value[0];
    if (history !== null) return null;
    const transaction = await this.rpc('getTransaction', [journal.signature, { commitment: 'finalized', encoding: 'base64', maxSupportedTransactionVersion: 0 }]);
    if (transaction !== null) return null;
    await this.assertNetwork();
    const { state } = await this.fetchedState(record);
    const before = intent.base.state;
    if (typeof before.nextNonce !== 'string' || typeof before.spentUnits !== 'string') return null;
    decimal(before.nextNonce, 'prior nextNonce'); decimal(before.spentUnits, 'prior spentUnits');
    if (state.nextNonce !== before.nextNonce || state.spentUnits !== before.spentUnits) return null;
    return { ...this.unknownReceipt(record, intent, journal), status: 'denied', state,
      error: { code: 'TRANSACTION_EXPIRED_NOT_LANDED', message: 'The signed transaction expired before landing. No transfer was settled.' },
      metadata: { outcome: 'expired_not_landed', expiryAudit: { finalizedBlockHeight: String(height), lastValidBlockHeight: expiry.toString(),
        historyStatus: null, finalizedTransaction: null, nextNonce: state.nextNonce, spentUnits: state.spentUnits, unchangedContractCounters: true } } };
  }
  async requestReceipts(record, state) {
    const directory = join(this.config.privateDirectory, 'requests');
    let files;
    try { files = await readdir(directory); } catch (error) { if (error.code === 'ENOENT') return []; throw error; }
    const results = [];
    for (const name of files) {
      if (!/^[0-9a-f]{64}\.json$/.test(name)) continue;
      const path = join(directory, name); const intent = await readJSON(path);
      if (intent.owner !== record.owner || intent.policyId !== record.policyId) continue;
      if (intent.result) { results.push(intent.result); continue; }
      const journal = intent.transactionId ? await readJSON(join(this.config.privateDirectory, 'transactions', `${intent.transactionId}.json`), null) : null;
      if (!journal?.signature) { results.push(this.unknownReceipt(record, intent, journal)); continue; }
      const status = (await this.rpc('getSignatureStatuses', [[journal.signature], { searchTransactionHistory: true }])).value[0];
      if (status?.confirmationStatus !== 'finalized') {
        const expired = status === null ? await this.expiredReceipt(record, intent, journal) : null;
        if (expired) {
          intent.signature = journal.signature; intent.status = 'expired_not_landed'; intent.result = expired;
          await durableJSON(path, intent);
          journal.status = 'expired_not_landed';
          await durableJSON(join(this.config.privateDirectory, 'transactions', `${intent.transactionId}.json`), journal);
          results.push(expired);
        } else results.push(this.unknownReceipt(record, intent, journal));
        continue;
      }
      const tx = await this.rpc('getTransaction', [journal.signature, { commitment: 'finalized', encoding: 'json', maxSupportedTransactionVersion: 0 }]);
      const wireReceipt = await this.rpc('getTransaction', [journal.signature, { commitment: 'finalized', encoding: 'base64', maxSupportedTransactionVersion: 0 }]);
      if (!tx || !wireReceipt || wireReceipt.transaction[0] !== journal.wire) throw new Error('recovery receipt differs from persisted signed executor transaction');
      const result = { ...intent.base, requestId: intent.requestId, requestHash: intent.requestHash, status: tx.meta?.err ? 'denied' : 'settled', chainVerified: true,
        signature: journal.signature, explorerUrl: `https://explorer.solana.com/tx/${journal.signature}?cluster=devnet`, state,
        error: tx.meta?.err ?? undefined, balanceDeltas: this.balanceDeltas(tx), receipt: { slot: String(tx.slot), err: tx.meta?.err, logMessages: tx.meta?.logMessages ?? [] } };
      if (!tx.meta?.err) {
        const from = result.balanceDeltas.find(delta => delta.accountAddress === record.sourceTokenAccount && delta.owner === record.owner);
        const to = result.balanceDeltas.find(delta => delta.accountAddress === record.destinationTokenAccount && delta.owner === record.mandate.recipient);
        if (!from || !to || BigInt(from.deltaUnits) !== -BigInt(intent.amountUnits) || BigInt(to.deltaUnits) !== BigInt(intent.amountUnits)) throw new Error('recovered transfer deltas differ from exact request');
      }
      intent.signature = journal.signature; intent.result = result; intent.status = tx.meta?.err ? 'finalized_failed' : 'finalized';
      await durableJSON(path, intent); results.push(result);
    }
    return results;
  }
  async state(body) {
    return this.queue.run(policyStorageKey(body.owner, body.policyId), () => this.stateUnlocked(body));
  }
  async stateUnlocked(body) {
    const record = await this.record(body.owner, body.policyId);
    const { state } = await this.fetchedState(record);
    const source = await this.token(record.sourceTokenAccount, record.owner);
    const destination = await this.token(record.destinationTokenAccount, record.mandate.recipient);
    return { ...this.binding(record), activationId: record.activationId, revocationId: record.revocationId,
      stateAddress: record.stateAddress, headAddress: record.headAddress, delegateAddress: record.delegateAddress,
      state, chainVerified: true, sourceBalanceUnits: source.amount.toString(), destinationBalanceUnits: destination.amount.toString(),
      delegatedAmountUnits: source.delegatedAmount.toString(), delegate: source.delegate ? decodeAddress(source.delegate) : null,
      requests: await this.requestReceipts(record, state) };
  }
  async activationVerify(body) {
    return this.queue.run(policyStorageKey(body.owner, body.policyId), async () => {
      const record = await this.record(body.owner, body.policyId);
      if (body.activationId !== record.activationId || !Array.isArray(body.signatures) || body.signatures.length !== record.phases.length) throw new Error('activation ID or signature count differs');
      for (let i=0;i<body.signatures.length;i++) {
        await this.waitFinalized(body.signatures[i], record.preparedTransactions[i].lastValidBlockHeight);
        await this.confirmedWire(body.signatures[i], record.preparedTransactions[i].transaction);
      }
      const result = await this.stateUnlocked(body);
      if (!result.state.active || result.state.revoked || result.delegate !== record.delegateAddress || BigInt(result.delegatedAmountUnits) !== BigInt(record.mandate.allocationUnits)) throw new Error('activation or exact delegated allowance is not finalized');
      record.status = 'active'; record.confirmedSignatures = body.signatures; await durableJSON(this.recordPath(record.owner, record.policyId), record);
      return { ...result, signatures: body.signatures, explorerUrls: body.signatures.map(signature => `https://explorer.solana.com/tx/${signature}?cluster=devnet`) };
    });
  }
  balanceDeltas(receipt) {
    const pre = receipt.meta?.preTokenBalances ?? []; const post = receipt.meta?.postTokenBalances ?? [];
    const keys = [...(receipt.transaction?.message?.accountKeys ?? []), ...(receipt.meta?.loadedAddresses?.writable ?? []), ...(receipt.meta?.loadedAddresses?.readonly ?? [])];
    return post.filter(balance => balance.mint === MINT).map(balance => {
      const before = pre.find(item => item.accountIndex === balance.accountIndex && item.mint === balance.mint)?.uiTokenAmount.amount ?? '0';
      const key = keys[balance.accountIndex];
      return { accountIndex: balance.accountIndex, accountAddress: typeof key === 'string' ? key : key?.pubkey, owner: balance.owner, mint: balance.mint, beforeUnits: before,
        afterUnits: balance.uiTokenAmount.amount, deltaUnits: (BigInt(balance.uiTokenAmount.amount) - BigInt(before)).toString() };
    });
  }
  liveReceipt(semantic, body, record, state, context, now, key, score) {
    const receipt = semantic.providerReceipt;
    if (!receipt || typeof receipt !== 'object' || Array.isArray(receipt)) throw new Error('live evidence requires the trusted gateway provider receipt');
    const m = state.mandate;
    const expected = { owner: record.owner, policyId: record.policyId, requestId: body.requestId, sourceHash: record.sourceHash, irHash: record.irHash,
      originalIntentHash: sha256(record.originalIntent), amountUnits: body.amountUnits, allocationUnits: m.allocationUnits, spentUnits: state.spentUnits,
      assessedNonce: state.nextNonce, action: m.action, merchant: m.merchant, recipient: m.recipient, token: 'USDC', network: NETWORK,
      question: semantic.question, evidenceKey: key, scoreBps: score };
    for (const [name, value] of Object.entries(expected)) if (receipt[name] !== value) throw new Error(`live assessment receipt has stale or mismatched ${name}`);
    if (receipt.provider !== 'typesafe' || receipt.model !== 'jev-1.13.0') throw new Error('live receipt is not from the configured Jev gateway model');
    if (!/^[0-9a-f]{64}$/.test(receipt.assessmentContextHash ?? '')) throw new Error('live assessment context hash is invalid');
    if (!receipt.runtimeContext || typeof receipt.runtimeContext !== 'object' || Array.isArray(receipt.runtimeContext) || canonical(receipt.runtimeContext) !== canonical(context)) throw new Error('live assessment runtime context differs from the exact request');
    const evaluatedAt = decimal(receipt.evaluatedAt, 'evaluatedAt');
    const observedAt = typeof receipt.observedAt === 'string' ? Date.parse(receipt.observedAt) / 1000 : NaN;
    if (evaluatedAt > BigInt(now+30) || evaluatedAt < BigInt(now-300) || !Number.isFinite(observedAt) || observedAt > now+30 || observedAt < now-300 || observedAt < Number(evaluatedAt)-30) throw new Error('live assessment receipt is not fresh');
    // Optional exact Go semanticState bytes let us check its serialization hash.
    // The authenticated gateway is the provider caller; this is not a separate
    // cryptographic signature from Jev and Node never contacts caller URLs.
    if (receipt.assessmentStateJSON !== undefined) {
      if (typeof receipt.assessmentStateJSON !== 'string' || Buffer.byteLength(receipt.assessmentStateJSON) > 8192 || sha256(receipt.assessmentStateJSON) !== receipt.assessmentContextHash) throw new Error('exact gateway assessment state hash differs');
      const assessed = JSON.parse(receipt.assessmentStateJSON);
      if (assessed.original_intent !== record.originalIntent || canonical(assessed.runtime_context) !== canonical(context) || canonical(assessed.owner_answers) !== '{}') throw new Error('exact gateway assessment state differs from mandate intent or runtime context');
      const request = assessed.request ?? {};
      for (const [name, value] of Object.entries({ amount_units: body.amountUnits, allocation_units: m.allocationUnits, spent_units: state.spentUnits, now: receipt.evaluatedAt })) {
        if (!Number.isSafeInteger(request[name]) || request[name] < 0 || String(request[name]) !== value) throw new Error(`exact gateway assessment state has mismatched ${name}`);
      }
      for (const name of ['action','merchant','recipient','token','network']) if (request[name] !== expected[name]) throw new Error(`exact gateway assessment state has mismatched ${name}`);
    }
    return { ...expected, provider: receipt.provider, model: receipt.model, assessmentContextHash: receipt.assessmentContextHash,
      runtimeContext: context, observedAt: receipt.observedAt, evaluatedAt: receipt.evaluatedAt,
      ...(receipt.assessmentStateJSON !== undefined ? { assessmentStateJSON: receipt.assessmentStateJSON } : {}) };
  }
  async execute(body) {
    return this.queue.run(policyStorageKey(body.owner, body.policyId), async () => {
      if (typeof body.requestId !== 'string' || !body.requestId || body.requestId.length > 128) throw new Error('bounded requestId is required');
      decimal(body.amountUnits);
      const record = await this.record(body.owner, body.policyId);
      if (body.recipient && body.recipient !== record.mandate.recipient) throw new Error('requested recipient differs from owner-approved mandate');
      const resultPath = join(this.config.privateDirectory, 'requests', `${sha256(`${record.owner}\n${record.policyId}\n${body.requestId}`)}.json`);
      const previous = await readJSON(resultPath, null);
      if (previous) {
        if (previous.inputHash !== sha256(JSON.stringify(body))) throw new Error('requestId already names different request details; execution refused');
        if (previous.result) return previous.result;
        const journal = previous.transactionId ? await readJSON(join(this.config.privateDirectory, 'transactions', `${previous.transactionId}.json`), null) : null;
        return this.unknownReceipt(record, { ...previous, requestId: body.requestId }, journal);
      }
      await this.assertNetwork();
      const { state, stateBase64 } = await this.fetchedState(record);
      const now = Math.floor(Date.now()/1000);
      const context = body.context ?? {};
      if (!context || typeof context !== 'object' || Array.isArray(context)) throw new Error('context must be a JSON object');
      validateContextNumbers(context);
      const intervals = [];
      let providerReceipt;
      const semantic = body.semanticEvidence && Object.keys(body.semanticEvidence).length ? body.semanticEvidence : null;
      if (semantic) {
        if (semantic.mode !== 'mock_explicit' && semantic.mode !== 'live') throw new Error('semantic evidence mode must be live or explicitly mock_explicit');
        if (semantic.mode === 'live' && this.config.allowLiveEvidence !== true) throw new Error('live gateway evidence is disabled by the explicit sidecar configuration');
        const key = (await this.codec({ op: 'semantic-key', question: semantic.question })).key;
        if (semantic.key && semantic.key !== key) throw new Error('semantic key differs from the exact question');
        const score = String(semantic.scoreBps); if (decimal(score, 'scoreBps') > 10000n) throw new Error('semantic BPS score exceeds 10000');
        if (semantic.mode === 'live') providerReceipt = this.liveReceipt(semantic, body, record, state, context, now, key, score);
        intervals.push({ name: key, lowerBps: score, upperBps: score });
      }
      const needEvidence = intervals.length > 0 || Object.keys(context).length > 0;
      const input = { op: 'execute', programId: this.config.programId, stateAddress: record.stateAddress, stateBase64,
        nonce: body.nonce ?? state.nextNonce, amountUnits: body.amountUnits, now: String(now), context,
        sourceTokenAccount: record.sourceTokenAccount, destinationTokenAccount: record.destinationTokenAccount,
        ...(needEvidence ? { evidence: { issuedAt: String(now), expiresAt: String(Math.min(now + 120, Number(state.mandate.expiresAt))), intervals } } : {}) };
      const prepared = await this.codec(input);
      const base = { ...this.binding(record), requestId: body.requestId, requestHash: prepared.requestHash, contextHash: prepared.contextHash, runtimeContext: prepared.runtimeContext,
        nonce: prepared.nonce, semanticMode: semantic ? semantic.mode : 'none', assessmentSource: semantic ? 'trusted_go_gateway' : 'none',
        ...(providerReceipt ? { providerReceipt } : {}), state, decision: prepared.preflight };
      const txId = `execute-${sha256(`${record.owner}\n${record.policyId}\n${body.requestId}`)}`;
      const intent = { owner: record.owner, policyId: record.policyId, requestId: body.requestId, inputHash: sha256(JSON.stringify(body)), nonce: prepared.nonce,
        amountUnits: body.amountUnits, base, transactionId: txId, requestHash: prepared.requestHash, status: 'prepared', mode: base.semanticMode };
      await durableJSON(resultPath, intent, true);
      if (!prepared.preflight.allowed && body.submitRejected !== true) {
        const result = { ...base, status: prepared.preflight.status, chainVerified: false, error: prepared.preflight.error };
        intent.result = result; intent.status = 'preflight_rejected'; await durableJSON(resultPath, intent); return result;
      }
      const signerKeys = [this.keys.executor];
      if (needEvidence && this.keys.evidence.address !== this.keys.executor.address) signerKeys.push(this.keys.evidence);
      let lookup = record.lookupAddress;
      try { await this.build([...prepared.computeBudgetInstructions, prepared.instruction], this.keys.executor.address, [], lookup); }
      catch (error) { if (!error.message.startsWith('transaction_packet_limit:')) throw error; lookup = await this.ensureLookup(record); }
      try {
        const submitted = await this.submit([...prepared.computeBudgetInstructions, prepared.instruction], signerKeys, txId, lookup, body.submitRejected === true);
        intent.signature = submitted.signature;
        const after = await this.fetchedState(record);
        const failed = submitted.receipt.meta?.err;
        if (!failed && (BigInt(after.state.nextNonce) !== BigInt(prepared.nonce)+1n || BigInt(after.state.spentUnits) !== BigInt(state.spentUnits)+BigInt(body.amountUnits))) throw new Error('finalized contract counters did not match exact spend and nonce');
        if (failed && (after.state.nextNonce !== state.nextNonce || after.state.spentUnits !== state.spentUnits)) throw new Error('failed transaction changed contract state');
        const deltas = this.balanceDeltas(submitted.receipt);
        if (failed && deltas.some(delta => BigInt(delta.deltaUnits) !== 0n)) throw new Error('failed transaction changed USDC token balances');
        if (!failed) {
          const from = deltas.find(delta => delta.accountAddress === record.sourceTokenAccount && delta.owner === record.owner); const to = deltas.find(delta => delta.accountAddress === record.destinationTokenAccount && delta.owner === record.mandate.recipient);
          if (!from || !to || BigInt(from.deltaUnits) !== -BigInt(body.amountUnits) || BigInt(to.deltaUnits) !== BigInt(body.amountUnits)) throw new Error('finalized token deltas did not match authorized transfer');
        }
        const result = { ...base, status: failed ? (prepared.preflight.status ?? 'denied') : 'settled', chainVerified: true,
          signature: submitted.signature, explorerUrl: `https://explorer.solana.com/tx/${submitted.signature}?cluster=devnet`,
          error: failed ? submitted.receipt.meta.err : undefined, state: after.state, balanceDeltas: deltas,
          receipt: { slot: String(submitted.receipt.slot), err: submitted.receipt.meta?.err, logMessages: submitted.receipt.meta?.logMessages ?? [] } };
        intent.status = failed ? 'finalized_failed' : 'finalized'; intent.result = result; await durableJSON(resultPath, intent);
        return result;
      } catch (error) {
        const journal = await readJSON(join(this.config.privateDirectory, 'transactions', `${txId}.json`), null);
        intent.status = journal?.status ?? 'not_submitted'; intent.signature = journal?.signature; intent.error = error.message;
        delete intent.result; // Unknown responses must remain recoverable, not cached as terminal receipts.
        await durableJSON(resultPath, intent);
        return this.unknownReceipt(record, intent, journal, error.message);
      }
    });
  }
  async revokePrepare(body) {
    await this.assertNetwork();
    const record = await this.record(body.owner, body.policyId); await this.fetchedState(record);
    if (!record.revocationId) {
      const instruction = await this.codec({ op: 'revoke', programId: this.config.programId, owner: body.owner, stateAddress: record.stateAddress });
      record.revocationId = randomUUID(); record.revocationTransaction = await this.build([...instruction.computeBudgetInstructions, instruction.instruction], body.owner);
      await durableJSON(this.recordPath(record.owner, record.policyId), record);
    }
    return { ...this.binding(record), revocationId: record.revocationId, transactions: [{ label: 'Permanently revoke this mandate', ...record.revocationTransaction }] };
  }
  async revokeVerify(body) {
    const record = await this.record(body.owner, body.policyId);
    if (record.revocationId !== body.revocationId || body.signatures?.length !== 1) throw new Error('revocation ID or signatures differ');
    await this.waitFinalized(body.signatures[0], record.revocationTransaction.lastValidBlockHeight);
    await this.confirmedWire(body.signatures[0], record.revocationTransaction.transaction);
    const result = await this.state(body);
    if (!result.state.revoked || result.state.active) throw new Error('revocation did not finalize');
    record.status = 'revoked'; await durableJSON(this.recordPath(record.owner, record.policyId), record);
    return { ...result, signatures: body.signatures };
  }
}
