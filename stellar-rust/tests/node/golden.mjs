// Independent official-SDK vectors. Test seeds are fixed public test material.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { Address, nativeToScVal, scValToNative, xdr, Networks, Keypair, Transaction } from '@stellar/stellar-sdk';
import { USDC_TESTNET_ADDRESS } from '@x402/stellar';

const bytes = n => Buffer.alloc(32, n);
const sha = b => crypto.createHash('sha256').update(b).digest();
const vault = Address.fromScAddress(xdr.ScAddress.scAddressTypeContract(bytes(7))).toString();
const recipient = Keypair.fromRawEd25519Seed(bytes(3)).publicKey();
const tuple = nativeToScVal([
  nativeToScVal('allowit_x402_v1', {type:'symbol'}),
  new Address(vault).toScVal(), nativeToScVal(bytes(8)),
  nativeToScVal(3n, {type:'u64'}), nativeToScVal(4n, {type:'u64'}),
  nativeToScVal(172900n, {type:'u64'}), nativeToScVal(bytes(9))
]);
const rootInvocation = new xdr.SorobanAuthorizedInvocation({
  function: xdr.SorobanAuthorizedFunction.sorobanAuthorizedFunctionTypeContractFn(
    new xdr.InvokeContractArgs({ contractAddress:new Address(USDC_TESTNET_ADDRESS).toScAddress(),
      functionName:'transfer',args:[new Address(vault).toScVal(),new Address(recipient).toScVal(),nativeToScVal(10000n,{type:'i128'})] })),
  subInvocations:[]
});
const result = { vault,recipient,asset:USDC_TESTNET_ADDRESS,customPreimage:tuple.toXDR('hex'),customDigest:sha(tuple.toXDR()).toString('hex') };
for (const [name,v2] of [['legacy',false],['v2',true]]) {
  const fields = {networkId:sha(Buffer.from(Networks.TESTNET)),nonce:xdr.Int64.fromString('12345'),signatureExpirationLedger:3456,invocation:rootInvocation};
  const preimage = v2
    ? xdr.HashIdPreimage.envelopeTypeSorobanAuthorizationWithAddress(new xdr.HashIdPreimageSorobanAuthorizationWithAddress({...fields,address:new Address(vault).toScAddress()}))
    : xdr.HashIdPreimage.envelopeTypeSorobanAuthorization(new xdr.HashIdPreimageSorobanAuthorization(fields));
  const credentials = new xdr.SorobanAddressCredentials({address:new Address(vault).toScAddress(),nonce:fields.nonce,signatureExpirationLedger:fields.signatureExpirationLedger,signature:xdr.ScVal.scvVoid()});
  const entry = new xdr.SorobanAuthorizationEntry({credentials:v2?xdr.SorobanCredentials.sorobanCredentialsAddressV2(credentials):xdr.SorobanCredentials.sorobanCredentialsAddress(credentials),rootInvocation});
  result[name] = {authorizationXdr:entry.toXDR('base64'),hostPreimage:preimage.toXDR('hex'),hostDigest:sha(preimage.toXDR()).toString('hex')};
}
const target = new URL('../fixtures/golden.json',import.meta.url);
if (process.argv.includes('--check')) {
  assert.deepEqual(JSON.parse(fs.readFileSync(target,'utf8')),result);
  const fixture = JSON.parse(fs.readFileSync(new URL('../fixtures/x402-vault.json',import.meta.url),'utf8'));
  const {config,prepared,signed} = fixture;
  const transaction = new Transaction(signed.transactionXdr,Networks.TESTNET);
  assert.equal(transaction.operations.length,1);
  const operation = transaction.operations[0];
  const invocation = operation.func.invokeContract();
  assert.equal(Address.fromScAddress(invocation.contractAddress()).toString(),config.asset);
  assert.equal(invocation.functionName().toString(),'transfer');
  assert.equal(scValToNative(invocation.args()[0]),config.vault);
  assert.equal(scValToNative(invocation.args()[1]),prepared.request.selectedOffer.payTo);
  assert.equal(scValToNative(invocation.args()[2]),10000n);
  assert.equal(operation.auth.length,1);
  const auth = operation.auth[0];
  const creds = auth.credentials().address();
  const host = sha(xdr.HashIdPreimage.envelopeTypeSorobanAuthorization(new xdr.HashIdPreimageSorobanAuthorization({
    networkId:sha(Buffer.from(Networks.TESTNET)),nonce:creds.nonce(),signatureExpirationLedger:creds.signatureExpirationLedger(),invocation:auth.rootInvocation()
  })).toXDR());
  assert.equal(host.toString('hex'),prepared.hostDigest);
  const custom = nativeToScVal([
    nativeToScVal('allowit_x402_v1',{type:'symbol'}),new Address(config.vault).toScVal(),nativeToScVal(host),
    nativeToScVal(BigInt(prepared.state.nonce),{type:'u64'}),nativeToScVal(BigInt(prepared.state.revision),{type:'u64'}),
    nativeToScVal(BigInt(prepared.expiresAt),{type:'u64'}),nativeToScVal(Buffer.from(prepared.requestHash,'hex'))
  ]);
  const digest=sha(custom.toXDR());
  assert.equal(digest.toString('hex'),prepared.authorizationDigest);
  const authorization = scValToNative(creds.signature());
  assert.equal(authorization.nonce,BigInt(prepared.state.nonce));
  assert.equal(authorization.revision,BigInt(prepared.state.revision));
  assert.equal(authorization.expires_at,BigInt(prepared.expiresAt));
  assert.equal(Buffer.from(authorization.request_hash).toString('hex'),prepared.requestHash);
  assert(Keypair.fromPublicKey(config.executor).verify(digest,Buffer.from(authorization.executor_signature)));
  assert(Keypair.fromPublicKey(config.authority).verify(digest,Buffer.from(authorization.authority_signature)));
  assert.deepEqual(JSON.parse(Buffer.from(signed.paymentSignature,'base64')),signed.payload);
  const confirmed = new Transaction(fixture.rpcTransaction.envelopeXdr,Networks.TESTNET);
  assert.equal(confirmed.hash().toString('hex'),fixture.receipt.transaction);
  assert.notEqual(confirmed.hash().toString('hex'),transaction.hash().toString('hex'));
  assert.equal(confirmed.operations[0].auth[0].toXDR('base64'),auth.toXDR('base64'));
  console.log('Official Stellar SDK validates Rust C-account auth, both signatures, payload and rewritten receipt.');
} else {
  fs.mkdirSync(new URL('../fixtures/',import.meta.url),{recursive:true});
  fs.writeFileSync(target,JSON.stringify(result,null,2)+'\n');
}
