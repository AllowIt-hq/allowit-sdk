import test from 'node:test';
import assert from 'node:assert/strict';
import { callSolanaRpc, rpcRetryDelay } from '../rpc.mjs';

const request = method => ({ jsonrpc: '2.0', id: 1, method, params: ['exact-bound-input'] });
const throttled = (status, retryAfter = '0') => ({ ok: false, status, headers: new Headers({ 'retry-after': retryAfter }), body: { cancel: async () => {} } });
const success = value => ({ ok: true, json: async () => ({ result: value }) });

test('a read recovers from HTTP 429 and 503 with identical request bytes', async () => {
  const sent = [], pauses = [];
  const responses = [throttled(429, '1'), throttled(503, '2'), success({ account: 'bound' })];
  const result = await callSolanaRpc('https://rpc.invalid', request('getAccountInfo'), {
    fetchImpl: async (_, options) => { sent.push(options.body); return responses.shift(); },
    pause: async ms => { pauses.push(ms); },
  });
  assert.deepEqual(result, { account: 'bound' }); assert.equal(sent.length, 3);
  assert(sent.every(body => body === sent[0])); assert.deepEqual(pauses, [1000, 2000]);
});

test('transaction submission and unlisted methods never retry HTTP failures', async () => {
  for (const method of ['sendTransaction', 'requestAirdrop', 'simulateTransaction', 'unknownRead']) {
    for (const status of [429, 503]) {
      let calls = 0;
      await assert.rejects(callSolanaRpc('https://rpc.invalid', request(method), {
        fetchImpl: async () => { calls++; return throttled(status); }, pause: async () => { throw new Error('must not wait'); },
      }), new RegExp(`RPC ${method} HTTP ${status}`));
      assert.equal(calls, 1, `${method} retried`);
    }
  }
});

test('persistent throttling stops at three attempts and two capped waits', async () => {
  let calls = 0; const pauses = [];
  await assert.rejects(callSolanaRpc('https://rpc.invalid', request('getTransaction'), {
    fetchImpl: async () => { calls++; return throttled(429, '600'); }, pause: async ms => { pauses.push(ms); },
  }), /RPC getTransaction HTTP 429/);
  assert.equal(calls, 3); assert.deepEqual(pauses, [10_000, 10_000]);
});

test('Retry-After supports HTTP dates, past dates and a ten-second maximum', () => {
  const now = Date.parse('2026-10-05T00:00:00Z');
  assert.equal(rpcRetryDelay('Mon, 05 Oct 2026 00:00:08 GMT', 1, now), 8000);
  assert.equal(rpcRetryDelay('Mon, 05 Oct 2026 00:01:00 GMT', 1, now), 10_000);
  assert.equal(rpcRetryDelay('Sun, 04 Oct 2026 23:59:59 GMT', 1, now), 0);
  assert.equal(rpcRetryDelay('invalid', 2, now), 2000);
});

test('the total read budget stops retries before a wait would exceed forty-five seconds', async () => {
  let clock = 0, calls = 0; const pauses = [];
  await assert.rejects(callSolanaRpc('https://rpc.invalid', request('getGenesisHash'), {
    now: () => clock, fetchImpl: async () => { calls++; clock += 20_000; return throttled(503, '10'); },
    pause: async ms => { pauses.push(ms); clock += ms; },
  }), /RPC getGenesisHash HTTP 503/);
  assert.equal(calls, 2); assert.deepEqual(pauses, [10_000]);
});

test('HTTP 500, transport errors and JSON-RPC errors are never retried', async () => {
  const cases = [async () => throttled(500), async () => { throw new Error('uncertain transport'); },
    async () => ({ ok: true, json: async () => ({ error: { code: -32002, message: 'binding rejected' } }) })];
  for (const reply of cases) {
    let calls = 0;
    await assert.rejects(callSolanaRpc('https://rpc.invalid', request('getAccountInfo'), {
      fetchImpl: async () => { calls++; return reply(); }, pause: async () => { throw new Error('must not wait'); },
    }));
    assert.equal(calls, 1);
  }
});

test('separate callers share serialized per-endpoint pacing, including submission', async () => {
  let clock = 0, active = 0, maximumActive = 0;
  const starts = [], delays = [];
  const options = { minimumIntervalMs: 500, now: () => clock,
    pause: async ms => { delays.push(ms); clock += ms; },
    fetchImpl: async (_, init) => {
      active++; maximumActive = Math.max(maximumActive, active);
      starts.push({ method: JSON.parse(init.body).method, time: clock });
      await Promise.resolve(); clock += 100; active--;
      return success('verified');
    } };
  const result = await Promise.all(['getAccountInfo', 'sendTransaction', 'getTransaction'].map(method =>
    callSolanaRpc('https://shared-pacing.invalid', request(method), { ...options })));
  assert.deepEqual(result, ['verified', 'verified', 'verified']);
  assert.deepEqual(starts, [{ method: 'getAccountInfo', time: 0 }, { method: 'sendTransaction', time: 500 }, { method: 'getTransaction', time: 1000 }]);
  assert.equal(maximumActive, 1); assert.deepEqual(delays, [400, 400]);
});

test('queue wait consumes the same deadline and an expired submission is never sent', async () => {
  let clock = 0, calls = 0;
  const options = { minimumIntervalMs: 500, now: () => clock, pause: async ms => { clock += ms; },
    fetchImpl: async () => { calls++; clock = 45_000; return success('read'); } };
  const first = callSolanaRpc('https://budget-pacing.invalid', request('getAccountInfo'), options);
  const second = assert.rejects(callSolanaRpc('https://budget-pacing.invalid', request('sendTransaction'), options), /pacing time budget exhausted/);
  assert.equal(await first, 'read'); await second; assert.equal(calls, 1);
});
