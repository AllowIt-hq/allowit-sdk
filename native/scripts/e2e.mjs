// Real RPC execution. Local validator mode is explicitly labelled and requires
// its actual genesis hash; it is never evidence of public Testnet acceptance.
import {readFile,writeFile,mkdir,chmod} from 'node:fs/promises';
import {resolve} from 'node:path';
import {Keypair,PublicKey,Transaction,TransactionInstruction,SystemProgram,Connection} from '@solana/web3.js';
import {NativePolicySDK,RELEASE} from '../index.mjs';
import {PolicyLifecycle} from '../lifecycle.mjs';
import {FileJournal} from '../journal.mjs';
import {TOKEN_PROGRAM_ID,getAssociatedTokenAddressSync,createAssociatedTokenAccountIdempotentInstruction} from '../token.mjs';
const rpc=process.env.ALLOWIT_RPC_URL??'https://api.testnet.solana.com',connection=new Connection(rpc,'finalized'),local=new URL(rpc).hostname==='127.0.0.1';
const directory=resolve(process.env.ALLOWIT_E2E_DIR??'.allowit-e2e');await mkdir(directory,{recursive:true,mode:0o700});
if(local&&await connection.getGenesisHash()!==process.env.ALLOWIT_LOCAL_GENESIS)throw Error('Explicit local validator genesis required');
if(!local&&await connection.getGenesisHash()!=='4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY')throw Error('Public E2E requires Solana Testnet');
const owner=Keypair.fromSecretKey(Uint8Array.from(JSON.parse(await readFile(process.env.ALLOWIT_OWNER_KEYPAIR,'utf8'))));
async function newKey(name){const path=directory+'/'+name+'.json';try{return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(await readFile(path,'utf8'))));}catch(e){if(e.code!=='ENOENT')throw e;const k=Keypair.generate();await writeFile(path,JSON.stringify(Array.from(k.secretKey)),{mode:0o600,flag:'wx'});return k;}}
const executor=await newKey('executor'),mint=await newKey('mint'),recipient=await newKey('recipient');
async function send(tx,signers=[owner]){const latest=await connection.getLatestBlockhash('finalized');tx.feePayer=signers[0].publicKey;tx.recentBlockhash=latest.blockhash;tx.sign(...signers);const signature=await connection.sendRawTransaction(tx.serialize());const receipt=await connection.confirmTransaction({...latest,signature},'finalized');if(receipt.value.err)throw Error(JSON.stringify(receipt.value.err));return signature;}
const ownerToken=getAssociatedTokenAddressSync(mint.publicKey,owner.publicKey),recipientToken=getAssociatedTokenAddressSync(mint.publicKey,recipient.publicKey);
if(!await connection.getAccountInfo(mint.publicKey,'finalized')){
 const init=new TransactionInstruction({programId:TOKEN_PROGRAM_ID,keys:[{pubkey:mint.publicKey,isSigner:false,isWritable:true}],data:Buffer.concat([Buffer.from([20,6]),owner.publicKey.toBuffer(),Buffer.from([0])])});
 const mintTo=Buffer.alloc(10);mintTo[0]=14;mintTo.writeBigUInt64LE(20_000_000n,1);mintTo[9]=6;
 await send(new Transaction().add(SystemProgram.createAccount({fromPubkey:owner.publicKey,newAccountPubkey:mint.publicKey,lamports:await connection.getMinimumBalanceForRentExemption(82),space:82,programId:TOKEN_PROGRAM_ID}),init,createAssociatedTokenAccountIdempotentInstruction(owner.publicKey,ownerToken,owner.publicKey,mint.publicKey),recipientATAInstruction(),new TransactionInstruction({programId:TOKEN_PROGRAM_ID,keys:[{pubkey:mint.publicKey,isSigner:false,isWritable:true},{pubkey:ownerToken,isSigner:false,isWritable:true},{pubkey:owner.publicKey,isSigner:true,isWritable:false}],data:mintTo}),SystemProgram.transfer({fromPubkey:owner.publicKey,toPubkey:executor.publicKey,lamports:50_000_000})),[owner,mint]);
}
function recipientATAInstruction(){return createAssociatedTokenAccountIdempotentInstruction(owner.publicKey,recipientToken,recipient.publicKey,mint.publicKey);}
const deployment=JSON.parse(await readFile(process.env.ALLOWIT_DEPLOYMENT_FILE,'utf8'));
const sdk=new NativePolicySDK({connection,rpcUrl:rpc,network:'solana:testnet',deployment,mint:mint.publicKey.toBase58(),executor:executor.publicKey.toBase58()});
if(local){sdk.checkNetwork=async()=>{if(await connection.getGenesisHash()!==process.env.ALLOWIT_LOCAL_GENESIS)throw Error('Local genesis changed');};sdk.transactionURL=signature=>`https://explorer.solana.com/tx/${signature}?cluster=custom&customUrl=${encodeURIComponent(rpc)}`;}
const policy=await sdk.generate('Spend up to 5 test tokens per day with PaySH discovery');
await writeFile(directory+'/policy.json',JSON.stringify(policy,null,2),{mode:0o600});
const journal=new FileJournal(directory+'/journal'),life=new PolicyLifecycle(sdk,journal,async(tx,role)=>{tx.sign(role==='owner'?owner:executor);return tx;});
const outcomes={network:local?'local-validator':'solana:testnet',genesis:await connection.getGenesisHash(),mint:mint.publicKey.toBase58(),owner:owner.publicKey.toBase58(),executor:executor.publicKey.toBase58(),recipientToken:recipientToken.toBase58(),deployment,policyId:policy.id};
async function run(method,options={},id=method+'-e2e-001'){
 let result=await life.submit(policy,owner.publicKey.toBase58(),method,options,id);const deadline=Date.now()+90_000;
 while(!['settled','failed'].includes(result.status)&&Date.now()<deadline){await new Promise(r=>setTimeout(r,500));result=await life.recover(id,policy,owner.publicKey.toBase58());}
 if(result.status!=='settled')throw Error(method+' did not settle: '+JSON.stringify({status:result.status,signature:result.signature}));
 const {signedBytes,intent,...publicResult}=result;return publicResult;
}
outcomes.deploy=await run('deploy');let state=await sdk.state(policy,owner.publicKey.toBase58());await writeFile(directory+'/SKILL.md',sdk.skill(policy,state),{mode:0o600});
outcomes.fund=await run('fund',{amount:'10'});outcomes.execute=await run('execute',{amount:'2',recipient:recipientToken.toBase58()});
const before=await sdk.state(policy,owner.publicKey.toBase58());outcomes.retry=await run('execute',{amount:'2',recipient:recipientToken.toBase58()});const after=await sdk.state(policy,owner.publicKey.toBase58());if(before.nonce!==after.nonce||before.balance!==after.balance||outcomes.retry.signature!==outcomes.execute.signature)throw Error('Replay duplicated spending');
try{await life.submit(policy,owner.publicKey.toBase58(),'execute',{amount:'4',recipient:recipientToken.toBase58()},'denied-e2e-001');throw Error('Expected daily ceiling denial');}catch(e){if(!/Daily limit exceeded/.test(e.message))throw e;outcomes.denied=e.message;}
// Bypass SDK checks to prove native Rust custody rejects over-limit execution.
state=await sdk.state(policy,owner.publicKey.toBase58());const malicious=new Transaction().add(sdk.instruction(state,'transfer',{amount:4_000_000n,destination:recipientToken.toBase58(),nonce:state.nonce,revision:state.revision}));
const latest=await connection.getLatestBlockhash('finalized');malicious.feePayer=executor.publicKey;malicious.recentBlockhash=latest.blockhash;malicious.sign(executor);const denied=await connection.simulateTransaction(malicious);if(!denied.value.err)throw Error('Native contract accepted over-limit spend');outcomes.nativeDenied=denied.value.err;outcomes.nativeDeniedLogs=denied.value.logs;
outcomes.pause=await run('tune',{amount:'0'});outcomes.revoke=await run('revoke');outcomes.withdraw=await run('withdraw',{amount:'8'});outcomes.final=await sdk.state(policy,owner.publicKey.toBase58());
if(outcomes.final.balance!=='0'||outcomes.final.spent!=='2000000'||outcomes.final.nonce!=='1'||outcomes.final.approved)throw Error('Final vault state mismatch');
await writeFile(directory+'/evidence.json',JSON.stringify(outcomes,null,2),{mode:0o600});console.log(JSON.stringify(outcomes,null,2));
