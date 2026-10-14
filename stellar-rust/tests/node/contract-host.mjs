// Published facilitator verification using a real compiled Soroban vault's
// successful host authorization and token event. RPC plumbing and fees are mocked.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { Address, Account, nativeToScVal, scValToNative, xdr, Networks, Keypair, StrKey, TransactionBuilder, Operation, rpc, Horizon, SorobanDataBuilder } from '@stellar/stellar-sdk';
import { createEd25519Signer } from '@x402/stellar';
import { ExactStellarScheme } from '@x402/stellar/exact/facilitator';

const p=JSON.parse(fs.readFileSync(new URL('../fixtures/contract-host.json',import.meta.url),'utf8'));
const sha=b=>crypto.createHash('sha256').update(b).digest();
assert.equal(p.networkPassphrase,Networks.TESTNET);
assert.equal(sha(Buffer.from(p.networkPassphrase)).toString('hex'),p.networkId);
const entry=xdr.SorobanAuthorizationEntry.fromXDR(p.entryXdrHex,'hex');
const c=entry.credentials().address();
const host=sha(xdr.HashIdPreimage.envelopeTypeSorobanAuthorization(new xdr.HashIdPreimageSorobanAuthorization({
  networkId:Buffer.from(p.networkId,'hex'),nonce:c.nonce(),signatureExpirationLedger:c.signatureExpirationLedger(),invocation:entry.rootInvocation()
})).toXDR());
assert.equal(host.toString('hex'),p.hostPayloadHex);
const digest=sha(nativeToScVal([
  nativeToScVal('allowit_x402_v1',{type:'symbol'}),new Address(p.from).toScVal(),nativeToScVal(host),
  nativeToScVal(BigInt(p.nonce),{type:'u64'}),nativeToScVal(BigInt(p.revision),{type:'u64'}),
  nativeToScVal(BigInt(p.expiresAt),{type:'u64'}),nativeToScVal(Buffer.from(p.requestHash,'hex'))
]).toXDR());
const auth=scValToNative(c.signature());
assert.equal(c.signature().toXDR('hex'),p.signatureXdrHex);
assert.equal(auth.nonce,BigInt(p.nonce));
assert.equal(auth.revision,BigInt(p.revision));
assert.equal(auth.expires_at,BigInt(p.expiresAt));
assert(Keypair.fromPublicKey(StrKey.encodeEd25519PublicKey(Buffer.from(p.executorPublicKey,'hex'))).verify(digest,Buffer.from(auth.executor_signature)));
assert(Keypair.fromPublicKey(StrKey.encodeEd25519PublicKey(Buffer.from(p.authorityPublicKey,'hex'))).verify(digest,Buffer.from(auth.authority_signature)));
const event=xdr.DiagnosticEvent.fromXDR(p.diagnosticEventXdrHex,'hex');
const requirements={scheme:'exact',network:'stellar:testnet',amount:p.amountToken,asset:p.asset,payTo:p.recipient,maxTimeoutSeconds:300,extra:{areFeesSponsored:true}};
const source=Keypair.fromRawEd25519Seed(Buffer.alloc(32,6));
const tx=new TransactionBuilder(new Account(StrKey.encodeEd25519PublicKey(Buffer.alloc(32)), '0'),{fee:'1100',networkPassphrase:Networks.TESTNET})
  .setTimeout(0).setSorobanData(new SorobanDataBuilder().setResourceFee('1000').build())
  .addOperation(Operation.invokeHostFunction({func:xdr.HostFunction.hostFunctionTypeInvokeContract(entry.rootInvocation().function().contractFn()),auth:[entry]})).build();
const payload={x402Version:2,resource:{url:'https://merchant.example/host-test'},accepted:requirements,payload:{transaction:tx.toXDR()}};
const saved={simulate:rpc.Server.prototype.simulateTransaction,latest:rpc.Server.prototype.getLatestLedger,ledgers:Horizon.Server.prototype.ledgers};
rpc.Server.prototype.simulateTransaction=async tx=> {
  assert.equal(tx.operations[0].auth[0].toXDR('hex'),p.entryXdrHex);
  return {_parsed:true,transactionData:new SorobanDataBuilder().setResourceFee('1000'),minResourceFee:'1000',events:[event],result:{auth:[],retval:xdr.ScVal.scvVoid()},latestLedger:p.latestLedger,id:'offline-contract-host'};
};
rpc.Server.prototype.getLatestLedger=async ()=>({sequence:p.latestLedger,id:'offline-contract-host',protocolVersion:28});
Horizon.Server.prototype.ledgers=function() {
  return {limit(){return this;},order(){return this;},async call(){return {records:[{closed_at:'2026-10-14T00:00:05Z'},{closed_at:'2026-10-14T00:00:00Z'}]};}};
};
try {
  const facilitator=new ExactStellarScheme([createEd25519Signer(source.secret(),'stellar:testnet')]);
  assert.deepEqual(await facilitator.verify(payload,requirements),{isValid:true,payer:p.from});
  console.log('Published x402 facilitator accepts the compiled-vault host authorization and real token event (offline RPC; fees mocked).');
} finally {
  rpc.Server.prototype.simulateTransaction=saved.simulate;
  rpc.Server.prototype.getLatestLedger=saved.latest;
  Horizon.Server.prototype.ledgers=saved.ledgers;
}
