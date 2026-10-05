import {test} from 'node:test';
import assert from 'node:assert/strict';
import {Keypair,PublicKey,Transaction,Connection} from '@solana/web3.js';
import {NativePolicySDK,units,decimal,encodeInstruction,decodeState,validatePolicy,RELEASE} from '../index.mjs';
import {PolicyLifecycle,base58,validateRecord} from '../lifecycle.mjs';
const owner=Keypair.generate(),executor=Keypair.generate();
const d={network:'solana:testnet',sourceBundle:RELEASE.sourceBundle,policy:Keypair.generate().publicKey.toBase58(),custody:Keypair.generate().publicKey.toBase58()};
d.policyData=PublicKey.findProgramAddressSync([new PublicKey(d.policy).toBuffer()],new PublicKey('BPFLoaderUpgradeab1e11111111111111111111111'))[0].toBase58();
const sdk=new NativePolicySDK({deployment:d,mint:Keypair.generate().publicKey.toBase58(),executor:executor.publicKey.toBase58()});
test('one-prompt generation binds source and effective parameters; refuses unsupported rules',async()=>{
 const p=await sdk.generate('Spend up to 5 test tokens per day');assert.equal(p.dailyLimit,'5');assert.notEqual((await sdk.generate(p.prompt)).id,p.id);assert.equal(p.rust,RELEASE.sources['policy.rs']);assert.equal(p.payDiscovery,false);
 await assert.rejects(sdk.generate('Spend up to 5 tokens per day only at one merchant'),/supports a daily/);
 await assert.rejects(sdk.generate('Spend up to 51 test tokens per day'),/ceiling/);await assert.rejects(validatePolicy({...p,dailyLimit:'6'}),/identity/);
 const pay=await sdk.generate('Spend up to 5 test tokens per day with PaySH discovery');const state={approved:true,sourceBundle:p.sourceBundle,policyArtifact:p.policyArtifact,mint:sdk.config.mint,dailyLimit:'5000000'};
 assert(!sdk.skill(p,state).includes('pay.sh'));assert(sdk.skill(pay,state).includes('Payment execution through PaySH is unavailable'));
});
test('amounts never pass through floats and native ABI matches Borsh golden bytes',()=>{
 assert.equal(units('1.000001'),1000001n);assert.equal(decimal(1000001n),'1.000001');for(const s of ['-1','1e6','0.0000001','01','1.'])assert.throws(()=>units(s));
 const ix=encodeInstruction('transfer',{amount:1n,nonce:2n,revision:3n});assert.equal(Buffer.from(ix).toString('hex'),'04010000000000000002000000000000000300000000000000');
 const state=Buffer.alloc(320);state[0]=1;state[298]=1;state.writeBigUInt64LE(5000000n,258);assert.equal(decodeState(state).dailyLimit,'5000000');state[299]=1;assert.throws(()=>decodeState(state),/state/);
});
test('wrong genesis refuses signing before release lookup',async()=>{const s=new NativePolicySDK({connection:{getGenesisHash:async()=> 'wrong'}});await assert.rejects(s.checkNetwork(),/genesis/);});
class MemoryJournal{values=new Map();async read(n){return structuredClone(this.values.get(n)??null);}async write(n,v){this.values.set(n,structuredClone(v));}async clear(n){this.values.delete(n);}async locked(f){return f();}}
async function fixture(){
 const p=await sdk.generate('Spend up to 5 test tokens per day'),b=sdk.publicBinding(p,owner.publicKey.toBase58());let sends=[],signs=0;
 const s=new NativePolicySDK({...sdk.config,connection:{sendRawTransaction:async raw=>{sends.push(Buffer.from(raw));throw Error('response lost');},getBlockHeight:async()=> 10}});
 s.verifyRelease=async()=>d;s.prepare=async()=>({transaction:new Transaction({blockhash:Keypair.generate().publicKey.toBase58(),lastValidBlockHeight:100,feePayer:executor.publicKey}).add(s.instruction(b,'transfer',{amount:1000000n,destination:owner.publicKey.toBase58(),nonce:'0',revision:'1'})),blockhash:null,lastValidBlockHeight:100,nonce:'0',revision:'1'});
 const original=s.prepare;s.prepare=async(...a)=>{const r=await original(...a);r.blockhash=r.transaction.recentBlockhash;return r;};s.status=async signature=>({status:'uncertain',signature,transactionUrl:s.transactionURL(signature)});
 const journal=new MemoryJournal(),life=new PolicyLifecycle(s,journal,async tx=>{signs++;tx.sign(executor);return tx;});
 return {p,s,journal,life,sends,signs:()=>signs};
}
test('uncertain retry reuses exact signed proof; blocks conflicting and additional spends',async()=>{
 const f=await fixture(),o=owner.publicKey.toBase58(),args={recipient:o,amount:'1'};
 const first=await f.life.submit(f.p,o,'execute',args,'same-request-001');assert.equal(first.status,'uncertain');
 const retry=await f.life.submit(f.p,o,'execute',args,'same-request-001');assert(retry.replayed);assert.equal(f.signs(),1);assert.equal(f.sends.length,2);assert(f.sends[0].equals(f.sends[1]));
 await assert.rejects(f.life.submit(f.p,o,'execute',{...args,amount:'2'},'same-request-001'),/conflict/);
 await assert.rejects(f.life.submit(f.p,o,'execute',args,'next-request-001'),/uncertain/);
 f.s.connection.getBlockHeight=async()=>101;f.s.connection.getSlot=async()=>99;f.s.connection.getBlock=async()=>({blockHeight:101});f.s.state=async()=>({nonce:'1',revision:'1'});await f.life.submit(f.p,o,'execute',args,'same-request-001');assert.equal(f.sends.length,2);
});
test('saved proof cannot be substituted with another signed transaction or configuration',async()=>{
 const f=await fixture(),o=owner.publicKey.toBase58();await f.life.submit(f.p,o,'execute',{recipient:o,amount:'1'},'integrity-request');
 const record=await f.journal.read('request-integrity-request');assert.doesNotThrow(()=>validateRecord(f.s,f.p,o,record));
 const tx=Transaction.from(Buffer.from(record.signedBytes,'base64'));tx.instructions[0].data=encodeInstruction('transfer',{amount:2n,nonce:0n,revision:1n});tx.sign(executor);
 await f.journal.write('request-integrity-request',{...record,signedBytes:tx.serialize().toString('base64'),signature:base58(tx.signature)});
 await assert.rejects(f.life.recover('integrity-request',f.p,o),/does not match/);assert.equal(f.sends.length,1);
});

test('classic SPL mint offsets and init state are decoded exactly',async()=>{const {getMint,TOKEN_PROGRAM_ID}=await import('../token.mjs');const data=Buffer.alloc(82);data[44]=6;data[45]=1;const connection={getAccountInfo:async()=>({owner:TOKEN_PROGRAM_ID,data})};assert.equal((await getMint(connection,owner.publicKey)).decimals,6);data[45]=0;await assert.rejects(getMint(connection,owner.publicKey),/initialized/);});

test('expired execute releases only with coherent unchanged nonce AND revision',async()=>{const f=await fixture(),o=owner.publicKey.toBase58();await f.life.submit(f.p,o,'execute',{recipient:o,amount:'1'},'expired-request-01');f.s.connection.getBlockHeight=async()=>101;f.s.connection.getSlot=async()=>99;f.s.connection.getBlock=async()=>({blockHeight:101});let got;f.s.state=async(p,owner,recovery,slot)=>{got=slot;return {nonce:'0',revision:'1'};};const r=await f.life.recover('expired-request-01',f.p,o);assert.equal(r.status,'failed');assert.equal(r.decisionCode,'EXPIRED_UNEXECUTED');assert.equal(got,99);});

test('amount spelling does not change operation identity',async()=>{const {intentFor}=await import('../lifecycle.mjs');const p=await sdk.generate('Spend up to 5 test tokens per day'),o=owner.publicKey.toBase58();assert.equal(intentFor(sdk,p,o,'fund',{amount:'2'}),intentFor(sdk,p,o,'fund',{amount:'2.0'}));const bundle=await sdk.bundle(p,o);assert.equal(bundle.context.owner,o);assert.equal(bundle.policy.id,p.id);assert(!JSON.stringify(bundle).includes('secretKey'));});

test('coherent expiry releases revoke/tune/deploy but never infers fund or withdrawal absence',async()=>{
 const {createAssociatedTokenAccountIdempotentInstruction,getAssociatedTokenAddressSync}=await import('../token.mjs');
 for(const method of ['revoke','tune','deploy','fund','withdraw']){
  const f=await fixture(),o=owner.publicKey.toBase58(),b=f.s.publicBinding(f.p,o),options=['tune','fund','withdraw'].includes(method)?{amount:'1'}:{};
  const intent=(await import('../lifecycle.mjs')).intentFor(f.s,f.p,o,method,options),blockhash=Keypair.generate().publicKey.toBase58(),rev='1';
  let instructions;
  if(method==='deploy')instructions=[createAssociatedTokenAccountIdempotentInstruction(owner.publicKey,new PublicKey(b.tokenAccount),new PublicKey(b.vault),new PublicKey(b.mint)),f.s.instruction(b,'initialize',{vaultId:f.p.id,dailyLimit:5000000n}),f.s.instruction(b,'approve',{approved:true,revision:0n})];
  if(method==='revoke')instructions=[f.s.instruction(b,'approve',{approved:false,revision:rev})];
  if(method==='tune')instructions=[f.s.instruction(b,'tune',{amount:1000000n,revision:rev})];
  if(method==='fund')instructions=[f.s.instruction(b,'deposit',{amount:1000000n,source:getAssociatedTokenAddressSync(new PublicKey(b.mint),owner.publicKey).toBase58()})];
  if(method==='withdraw'){const dest=getAssociatedTokenAddressSync(new PublicKey(b.mint),owner.publicKey);instructions=[createAssociatedTokenAccountIdempotentInstruction(owner.publicKey,dest,owner.publicKey,new PublicKey(b.mint)),f.s.instruction(b,'withdraw',{amount:1000000n,destination:dest.toBase58()})];}
  const tx=new Transaction({feePayer:owner.publicKey,blockhash,lastValidBlockHeight:100}).add(...instructions);tx.sign(owner);
  const record={id:'expiry-'+method,intent,method,status:'uncertain',signature:base58(tx.signature),signedBytes:tx.serialize().toString('base64'),blockhash,lastValidBlockHeight:100,revision:rev,nonce:'0'};
  await f.journal.write('request-'+record.id,record);f.s.connection.getBlockHeight=async()=>101;f.s.connection.getSlot=async()=>99;f.s.connection.getBlock=async()=>({blockHeight:101});f.s.state=async(p,o,recovery,slot)=>{assert.equal(slot,99);return method==='deploy'?null:{revision:rev,nonce:'0'};};
  const recovered=await f.life.recover(record.id,f.p,o);assert.equal(recovered.status,['fund','withdraw'].includes(method)?'uncertain':'failed');
  if(['fund','withdraw'].includes(method))assert.equal(recovered.blockhashExpired,true);
  else {f.s.state=async()=>{throw Error('Must reuse persisted absence evidence');};assert.equal((await f.life.recover(record.id,f.p,o)).status,'failed');}
 }
});

test('CLI imports the exact browser instance without any key or RPC; status never loads owner key',async()=>{
 const {mkdtemp,writeFile,readFile,rm}=await import('node:fs/promises'),{tmpdir}=await import('node:os'),{join}=await import('node:path'),{execFile}=await import('node:child_process'),{promisify}=await import('node:util'),{createServer}=await import('node:http');
 const run=promisify(execFile),directory=await mkdtemp(join(tmpdir(),'allowit-import-')),p=await sdk.generate('Spend up to 5 test tokens per day'),bundle=await sdk.bundle(p,owner.publicKey.toBase58());
 const server=createServer((req,res)=>{let body='';req.on('data',chunk=>body+=chunk);req.on('end',()=>{res.setHeader('Content-Type','application/json');res.end(JSON.stringify({jsonrpc:'2.0',id:JSON.parse(body).id,result:'wrong-genesis'}));});});await new Promise(r=>server.listen(0,'127.0.0.1',r));
 try{
  const file=join(directory,'executor.json');await writeFile(file,JSON.stringify(bundle));
  const env={PATH:process.env.PATH,ALLOWIT_POLICY_DIR:join(directory,'policy'),ALLOWIT_OWNER_KEYPAIR:'/nonexistent-owner-key',ALLOWIT_EXECUTOR_KEYPAIR:'/nonexistent-executor-key',ALLOWIT_RPC_URL:'http://127.0.0.1:'+server.address().port};
  const output=await run(process.execPath,[new URL('../cli.mjs',import.meta.url).pathname,'import',file,'--json'],{env});assert.equal(JSON.parse(output.stdout).policyId,p.id);assert.deepEqual(JSON.parse(await readFile(join(directory,'policy/policy.json'),'utf8')),p);
  await assert.rejects(run(process.execPath,[new URL('../cli.mjs',import.meta.url).pathname,'status'],{env}),e=>e.code===3&&/genesis/.test(e.stderr)&&!/key file|ENOENT/.test(e.stderr));
  await assert.rejects(run(process.execPath,[new URL('../cli.mjs',import.meta.url).pathname,'import',file],{env}),e=>e.code===3&&/already exists/.test(e.stderr));
 }finally{server.close();await rm(directory,{recursive:true,force:true});}
});
