import { createHash, timingSafeEqual, randomUUID } from 'node:crypto';
import { open, mkdir, rename, readFile } from 'node:fs/promises';
import { resolve, relative, isAbsolute, dirname, sep } from 'node:path';

export const NETWORK = 'solana:devnet';
export const GENESIS_HASH = 'EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG';
export const MINT = '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU';
export const TOKEN_PROGRAM = 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA';
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export function decimal(value, label = 'amountUnits') {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value)) throw new Error(`${label} must be a decimal string`);
  const number = BigInt(value);
  if (number > (1n << 64n) - 1n) throw new Error(`${label} exceeds u64`);
  return number;
}
export function validateContextNumbers(value, depth = 0) {
  if (depth > 8) throw new Error('demo runtime context exceeds depth 8');
  if (typeof value === 'number' && !Number.isSafeInteger(value)) throw new Error('demo context numbers must be exact safe integers');
  if (value !== null && typeof value === 'object') for (const child of Object.values(value)) validateContextNumbers(child, depth+1);
}
export function bearerMatches(received, expected) {
  const a = Buffer.from(received ?? ''); const b = Buffer.from(`Bearer ${expected}`);
  return a.length === b.length && timingSafeEqual(a, b);
}
export function outsideRepository(candidate, repository) {
  const path = resolve(candidate); const rel = relative(resolve(repository), path);
  if (!rel || (rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel))) throw new Error('private files must be outside the repository');
  return path;
}
export async function durableJSON(path, value, exclusive = false) {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = exclusive ? path : `${path}.tmp.${randomUUID()}`;
  const file = await open(temporary, exclusive ? 'wx' : 'w', 0o600);
  try { await file.writeFile(JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v)); await file.sync(); } finally { await file.close(); }
  if (!exclusive) await rename(temporary, path);
}
export async function readJSON(path, fallback) {
  try { return JSON.parse(await readFile(path, 'utf8')); } catch (error) { if (error.code === 'ENOENT' && fallback !== undefined) return fallback; throw error; }
}
export function policyStorageKey(owner, policyId) { return sha256(`${owner}\n${policyId}`); }
export function tokenAccountFields(base64) {
  const data = Buffer.from(base64, 'base64');
  if (data.length !== 165) throw new Error('expected classic SPL Token account');
  const delegateTag = data.readUInt32LE(72);
  if (delegateTag !== 0 && delegateTag !== 1) throw new Error('invalid token delegate option');
  return { mint: data.subarray(0, 32), owner: data.subarray(32, 64), amount: data.readBigUInt64LE(64),
    delegate: delegateTag ? data.subarray(76, 108) : null, delegatedAmount: data.readBigUInt64LE(121), state: data[108] };
}
export class SerialQueue {
  #tails = new Map();
  async run(key, action) {
    const previous = this.#tails.get(key) ?? Promise.resolve();
    let release; const gate = new Promise(resolve => { release = resolve; });
    const tail = previous.catch(() => {}).then(() => gate);
    this.#tails.set(key, tail);
    await previous.catch(() => {});
    try { return await action(); } finally { release(); if (this.#tails.get(key) === tail) this.#tails.delete(key); }
  }
}
