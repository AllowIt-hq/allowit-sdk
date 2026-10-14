// Executes the published facilitator verifier against deterministic offline RPC responses.
// This proves protocol interoperability; it does not represent a network settlement.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { Address, nativeToScVal, scValToNative, xdr, Networks, Keypair, Transaction, rpc, Horizon, SorobanDataBuilder } from '@stellar/stellar-sdk';
import { createEd25519Signer } from '@x402/stellar';
import { ExactStellarScheme } from '@x402/stellar/exact/facilitator';

const f=JSON.parse(fs.readFileSync(new URL('../fixtures/x402-vault.json',import.meta.url),'utf8'));
const sha=b=>crypto.createHash('sha256').update(b).digest();
const key=n=>Keypair.fromRawEd25519Seed(Buffer.alloc(32,n));
const preimage=(entry)=> {
  const v2=entry.credentials().switch().name==='sorobanCredentialsAddressV2';
  const c=v2?entry.credentials().addressV2():entry.credentials().address();
  const fields={networkId:sha(Buffer.from(Networks.TESTNET)),nonce:c.nonce(),signatureExpirationLedger:c.signatureExpirationLedger(),invocation:entry.rootInvocation()};
  return v2
    ? xdr.HashIdPreimage.envelopeTypeSorobanAuthorizationWithAddress(new xdr.HashIdPreimageSorobanAuthorizationWithAddress({...fields,address:c.address()})).toXDR()
    : xdr.HashIdPreimage.envelopeTypeSorobanAuthorization(new xdr.HashIdPreimageSorobanAuthorization(fields)).toXDR();
};
const customDigest=host=>sha(nativeToScVal([
  nativeToScVal('allowit_x402_v1',{type:'symbol'}),new Address(f.config.vault).toScVal(),nativeToScVal(host),
  nativeToScVal(BigInt(f.prepared.state.nonce),{type:'u64'}),nativeToScVal(BigInt(f.prepared.state.revision),{type:'u64'}),
  nativeToScVal(BigInt(f.prepared.expiresAt),{type:'u64'}),nativeToScVal(Buffer.from(f.prepared.requestHash,'hex'))
]).toXDR());

const saved={simulate:rpc.Server.prototype.simulateTransaction,latest:rpc.Server.prototype.getLatestLedger,ledgers:Horizon.Server.prototype.ledgers};
let resourceFee='1000';
const event=xdr.TransactionMeta.fromXDR(f.rpcTransaction.resultMetaXdr,'base64').v4().operations()[0].events()[0];
rpc.Server.prototype.simulateTransaction=async function(transaction) {
  try {
    const auth=transaction.operations[0].auth[0];
    const c=auth.credentials().switch().name==='sorobanCredentialsAddressV2'?auth.credentials().addressV2():auth.credentials().address();
    const signature=scValToNative(c.signature());
    assert.equal(Address.fromScAddress(c.address()).toString(),f.config.vault);
    assert.equal(signature.nonce,BigInt(f.prepared.state.nonce));
    assert.equal(signature.revision,BigInt(f.prepared.state.revision));
    assert.equal(signature.expires_at,BigInt(f.prepared.expiresAt));
    assert.equal(Buffer.from(signature.request_hash).toString('hex'),f.prepared.requestHash);
    const digest=customDigest(sha(preimage(auth)));
    assert(key(2).verify(digest,Buffer.from(signature.executor_signature)));
    assert(key(4).verify(digest,Buffer.from(signature.authority_signature)));
  } catch {
    return {error:'Offline RPC auth model rejected signature or policy binding',latestLedger:3444,id:'offline'};
  }
  return {
    _parsed:true,
    transactionData:new SorobanDataBuilder(f.transactionData),minResourceFee:resourceFee,
    events:[new xdr.DiagnosticEvent({inSuccessfulContractCall:true,event})],
    result:{auth:[],retval:xdr.ScVal.scvVoid()},latestLedger:3444,id:'offline'
  };
};
rpc.Server.prototype.getLatestLedger=async ()=>({sequence:3444,id:'offline',protocolVersion:28});
Horizon.Server.prototype.ledgers=function() {
  return {limit(){return this;},order(){return this;},async call(){return {records:[{closed_at:'2026-10-14T00:00:05Z'},{closed_at:'2026-10-14T00:00:00Z'}]};}};
};

try {
  const facilitator=new ExactStellarScheme([createEd25519Signer(key(6).secret(),'stellar:testnet')]);
  assert.deepEqual(await facilitator.verify(f.signed.payload,f.selectedOffer),{isValid:true,payer:f.config.vault});

  const tx=new Transaction(f.signed.transactionXdr,Networks.TESTNET);
  const old=tx.operations[0].auth[0];
  const creds=old.credentials().address();
  const entry=new xdr.SorobanAuthorizationEntry({credentials:xdr.SorobanCredentials.sorobanCredentialsAddressV2(creds),rootInvocation:old.rootInvocation()});
  const digest=customDigest(sha(preimage(entry)));
  creds.signature(nativeToScVal({
    authority_signature:key(4).sign(digest),executor_signature:key(2).sign(digest),
    expires_at:nativeToScVal(BigInt(f.prepared.expiresAt),{type:'u64'}),
    nonce:nativeToScVal(BigInt(f.prepared.state.nonce),{type:'u64'}),
    request_hash:Buffer.from(f.prepared.requestHash,'hex'),revision:nativeToScVal(BigInt(f.prepared.state.revision),{type:'u64'})
  }));
  const env=xdr.TransactionEnvelope.fromXDR(f.signed.transactionXdr,'base64');
  env.v1().tx().operations()[0].body().invokeHostFunctionOp().auth([entry]);
  const v2=structuredClone(f.signed.payload);
  v2.payload.transaction=env.toXDR('base64');
  assert.deepEqual(await facilitator.verify(v2,f.selectedOffer),{isValid:true,payer:f.config.vault});

  const wrongOffer={...f.selectedOffer,amount:'10010'};
  const wrongAmount=await facilitator.verify(f.signed.payload,wrongOffer);
  assert.equal(wrongAmount.isValid,false);
  assert.equal(wrongAmount.invalidReason,'invalid_exact_stellar_payload_wrong_amount');
  resourceFee='50001';
  const excessive=await facilitator.verify(f.signed.payload,f.selectedOffer);
  assert.equal(excessive.isValid,false);
  assert.equal(excessive.invalidReason,'invalid_exact_stellar_payload_fee_exceeds_maximum');
  resourceFee='1000';
  const corrupted=xdr.TransactionEnvelope.fromXDR(v2.payload.transaction,'base64');
  corrupted.v1().tx().operations()[0].body().invokeHostFunctionOp().auth()[0].credentials().addressV2().signature(xdr.ScVal.scvVoid());
  const unsigned=structuredClone(v2); unsigned.payload.transaction=corrupted.toXDR('base64');
  assert.equal((await facilitator.verify(unsigned,f.selectedOffer)).isValid,false);
  console.log('Published @x402/stellar facilitator accepts both C-account credential arms; rejects wrong amount, missing signature and excess fee (offline RPC).');
} finally {
  rpc.Server.prototype.simulateTransaction=saved.simulate;
  rpc.Server.prototype.getLatestLedger=saved.latest;
  Horizon.Server.prototype.ledgers=saved.ledgers;
}
