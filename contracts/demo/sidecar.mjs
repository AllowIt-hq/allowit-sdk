import { createServer } from 'node:http';
import { Driver } from './driver.mjs';
import { bearerMatches } from './safety.mjs';

const configPath = process.env.ALLOWIT_DEMO_CONFIG;
if (!configPath) throw new Error('ALLOWIT_DEMO_CONFIG must name an external private JSON configuration');
const driver = await Driver.load(configPath);
const routes = new Map([
  ['GET /v1/info', () => driver.info()],
  ['POST /v1/info', () => driver.info()],
  ['POST /v1/activation/prepare', body => driver.activationPrepare(body)],
  ['POST /v1/activation/verify', body => driver.activationVerify(body)],
  ['POST /v1/transactions/confirm', body => driver.confirm(body)],
  ['POST /v1/transactions/refresh', body => driver.refreshTransaction(body)],
  ['POST /v1/requests/execute', body => driver.execute(body)],
  ['GET /v1/state', body => driver.state(body)],
  ['POST /v1/state', body => driver.state(body)],
  ['POST /v1/revoke/prepare', body => driver.revokePrepare(body)],
  ['POST /v1/revoke/verify', body => driver.revokeVerify(body)],
]);
const server = createServer(async (request, response) => {
  response.setHeader('content-type', 'application/json'); response.setHeader('cache-control', 'no-store');
  if (!bearerMatches(request.headers.authorization, driver.config.bearerToken)) { response.writeHead(401); response.end(JSON.stringify({ error: 'unauthorized' })); return; }
  try {
    const url = new URL(request.url, 'http://127.0.0.1');
    const path = url.pathname.startsWith('/v1/') ? url.pathname : `/v1${url.pathname}`;
    const handler = routes.get(`${request.method} ${path}`);
    if (!handler) { response.writeHead(404); response.end(JSON.stringify({ error: 'unknown endpoint' })); return; }
    let body = Object.fromEntries(url.searchParams);
    if (request.method === 'POST') {
      const chunks = []; let size = 0;
      for await (const chunk of request) { size += chunk.length; if (size > 64 * 1024) throw new Error('request body exceeds 64KiB'); chunks.push(chunk); }
      body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    }
    const result = await handler(body);
    response.end(JSON.stringify(result, (_, value) => typeof value === 'bigint' ? value.toString() : value));
  } catch (error) {
    // Keep private gateway credentials and request bodies out of diagnostics.
    console.error(JSON.stringify({ event: 'devnet_operation_failed', method: request.method,
      path: new URL(request.url, 'http://127.0.0.1').pathname, error: error.message }));
    response.writeHead(422); response.end(JSON.stringify({ error: error.message }));
  }
});
server.requestTimeout = 180_000;
server.listen(driver.config.port ?? 4318, '127.0.0.1', () => console.log(`AllowIt Devnet sidecar listening on 127.0.0.1:${driver.config.port ?? 4318}`));
