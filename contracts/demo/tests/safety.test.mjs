import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { bearerMatches, decimal, outsideRepository, tokenAccountFields, durableJSON, validateContextNumbers, SerialQueue } from '../safety.mjs';

test('integer amounts stay exact and malformed or unsafe inputs are rejected', () => {
  assert.equal(decimal('9007199254740993'), 9007199254740993n);
  for (const value of [3, '03', '-1', '1.5', '18446744073709551616']) assert.throws(() => decimal(value));
});
test('demo runtime context only permits exactly representable integers, including nested values', () => {
  validateContextNumbers({ candidate_yield_bps: 800, benchmark_yield_bps: 850, claims: ['text', 9007199254740991, -9007199254740991] });
  for (const number of [9007199254740992, -9007199254740992, 0.1, NaN, Infinity]) assert.throws(() => validateContextNumbers({ nested: [number] }), /exact safe integers/);
});
test('private storage cannot be located in the checked-out source', () => {
  assert.throws(() => outsideRepository('/repo/private', '/repo'));
  assert.throws(() => outsideRepository('/repo/..private', '/repo'));
  assert.throws(() => outsideRepository('/repo', '/repo'));
  assert.equal(outsideRepository('/private', '/repo'), resolve('/private'));
  assert.equal(bearerMatches('Bearer secret', 'secret'), true);
  assert.equal(bearerMatches('Bearer wrong', 'secret'), false);
});
test('classic token delegate allowance is read from canonical account offsets', () => {
  const data = Buffer.alloc(165); data.writeBigUInt64LE(99n, 64); data.writeUInt32LE(1, 72); data.fill(4, 76, 108); data[108] = 1; data.writeBigUInt64LE(81n, 121);
  const fields = tokenAccountFields(data.toString('base64'));
  assert.equal(fields.amount, 99n); assert.equal(fields.delegatedAmount, 81n); assert.deepEqual(fields.delegate, Buffer.alloc(32, 4));
  assert.throws(() => tokenAccountFields(Buffer.alloc(166).toString('base64')));
});
test('execution queue serializes the nonce critical section, including a rejected predecessor', async () => {
  const queue = new SerialQueue(); const events = [];
  const a = queue.run('mandate', async () => { events.push('a-start'); await new Promise(resolve => setTimeout(resolve, 20)); events.push('a-end'); throw new Error('a failed'); });
  const b = queue.run('mandate', async () => { events.push('b'); });
  await assert.rejects(a); await b; assert.deepEqual(events, ['a-start', 'a-end', 'b']);
});
test('durable signed-wire journal exists before a simulated transport send and refuses duplicate ID', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'allowit-journal-'));
  try {
    const path = join(directory, 'signed.json');
    await durableJSON(path, { signature: 'fixture', wire: 'base64', nonce: '0' }, true);
    const sent = JSON.parse(await readFile(path, 'utf8')); assert.equal(sent.wire, 'base64');
    await assert.rejects(durableJSON(path, { wire: 'second' }, true), /EEXIST/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});
