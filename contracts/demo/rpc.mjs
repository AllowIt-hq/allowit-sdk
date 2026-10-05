// Retrying a read is safe; an uncertain transaction submission must be queried.
const READ_METHODS = new Set([
  'getGenesisHash', 'getAccountInfo', 'getSlot', 'getLatestBlockhash',
  'getSignatureStatuses', 'getTransaction', 'getBlockHeight',
  'getMinimumBalanceForRentExemption',
]);
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
// All Driver instances in this process share one pacing queue per endpoint.
// A submission participates in pacing but still receives exactly one attempt.
const endpointQueues = new Map();

async function pacedAttempt(url, minimumIntervalMs, deadline, { now, pause }, action) {
  if (minimumIntervalMs === 0) return action();
  let queue = endpointQueues.get(url);
  if (!queue) { queue = { tail: Promise.resolve(), lastStartedAt: null }; endpointQueues.set(url, queue); }
  const previous = queue.tail;
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  queue.tail = previous.catch(() => {}).then(() => gate);
  await previous.catch(() => {});
  try {
    const delay = queue.lastStartedAt === null ? 0 : Math.max(0, minimumIntervalMs - (now() - queue.lastStartedAt));
    if (deadline - now() <= delay) throw new Error('RPC pacing time budget exhausted');
    if (delay) await pause(delay);
    if (deadline <= now()) throw new Error('RPC pacing time budget exhausted');
    queue.lastStartedAt = now();
    return await action();
  } finally { release(); }
}

export function rpcRetryDelay(retryAfter, attempt, now = Date.now()) {
  let delay = Math.min(attempt * 1000, 10_000);
  if (typeof retryAfter === 'string' && /^\s*\d+\s*$/.test(retryAfter)) delay = Number(retryAfter) * 1000;
  else if (typeof retryAfter === 'string' && Number.isFinite(Date.parse(retryAfter))) delay = Date.parse(retryAfter) - now;
  return Math.max(0, Math.min(delay, 10_000));
}

export async function callSolanaRpc(url, request, { fetchImpl = fetch, pause = wait, now = Date.now, minimumIntervalMs = 0 } = {}) {
  if (!Number.isSafeInteger(minimumIntervalMs) || minimumIntervalMs < 0 || minimumIntervalMs > 10_000) throw new Error('invalid RPC pacing interval');
  const deadline = now() + 45_000;
  const body = JSON.stringify(request);
  const attempts = READ_METHODS.has(request.method) ? 3 : 1;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    const remaining = deadline - now();
    if (remaining <= 0) throw new Error(`RPC ${request.method}: read retry time budget exhausted`);
    // Transport/time-out failures are deliberately not caught or retried.
    const response = await pacedAttempt(url, minimumIntervalMs, deadline, { now, pause }, () => {
      const budget = deadline - now();
      if (budget <= 0) throw new Error(`RPC ${request.method}: pacing time budget exhausted`);
      return fetchImpl(url, { method: 'POST', headers: { 'content-type': 'application/json' },
        body, signal: AbortSignal.timeout(Math.min(20_000, budget)) });
    });
    if (!response.ok) {
      const error = new Error(`RPC ${request.method} HTTP ${response.status}`);
      if (attempt === attempts || ![429, 503].includes(response.status)) throw error;
      const delay = rpcRetryDelay(response.headers.get('retry-after'), attempt, now());
      if (deadline - now() <= delay) throw error;
      // Release the throttled response before waiting, rather than retain its socket.
      await response.body?.cancel();
      await pause(delay);
      continue;
    }
    const payload = await response.json();
    if (payload.error) {
      const error = new Error(`RPC ${request.method}: ${payload.error.message}`);
      error.rpcError = payload.error;
      throw error;
    }
    return payload.result;
  }
}
