import {Buffer} from 'buffer';
import {Transaction,PublicKey} from '@solana/web3.js';
import {digest,validatePolicy,units,decimal} from './index.mjs';
import {getAssociatedTokenAddressSync,createAssociatedTokenAccountIdempotentInstruction} from './token.mjs';
const encoder=new TextEncoder();
export function base58(data){const alphabet='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';let n=0n;for(const b of data)n=n*256n+BigInt(b);let text='';while(n){text=alphabet[Number(n%58n)]+text;n/=58n;}for(const b of data){if(b)break;text='1'+text;}return text;}
export function intentFor(sdk,policy,owner,method,options={}){
 const b=sdk.publicBinding(policy,owner);
 return JSON.stringify({policyId:policy.id,network:policy.network,owner,method,amount:options.amount===undefined?null:decimal(units(options.amount)),recipient:options.recipient??null,binding:b});
}
function expectedInstructions(sdk,policy,b,method,options,record){
 const k=v=>new PublicKey(v), amount=options.amount===undefined?undefined:units(options.amount);
 switch(method){
 case 'deploy':return [createAssociatedTokenAccountIdempotentInstruction(k(b.owner),k(b.tokenAccount),k(b.vault),k(b.mint)),sdk.instruction(b,'initialize',{vaultId:policy.id,dailyLimit:units(policy.dailyLimit)}),sdk.instruction(b,'approve',{approved:true,revision:0n})];
 case 'fund':return [sdk.instruction(b,'deposit',{amount,source:getAssociatedTokenAddressSync(k(b.mint),k(b.owner)).toBase58()})];
 case 'execute':return [sdk.instruction(b,'transfer',{amount,destination:options.recipient,nonce:record.nonce,revision:record.revision})];
 case 'revoke':return [sdk.instruction(b,'approve',{approved:false,revision:record.revision})];
 case 'tune':return [sdk.instruction(b,'tune',{amount,revision:record.revision})];
 case 'withdraw':{const dest=getAssociatedTokenAddressSync(k(b.mint),k(b.owner));return [createAssociatedTokenAccountIdempotentInstruction(k(b.owner),dest,k(b.owner),k(b.mint)),sdk.instruction(b,'withdraw',{amount,destination:dest.toBase58()})];}
 default:throw Error('Unsupported persisted operation');
 }
}
/** Validates saved proof against caller policy/configuration before status or rebroadcast. */
export function validateRecord(sdk,policy,owner,record){
 if(!record||typeof record.intent!=='string'||!record.signedBytes||!Number.isSafeInteger(record.lastValidBlockHeight))throw Error('Invalid operation journal');
 const intent=JSON.parse(record.intent),options={};if(intent.amount!==null)options.amount=intent.amount;if(intent.recipient!==null)options.recipient=intent.recipient;
 if(intentFor(sdk,policy,owner,record.method,options)!==record.intent||record.method!==intent.method)throw Error('Operation binding changed');
 const b=sdk.publicBinding(policy,owner),expected=new Transaction({feePayer:new PublicKey(record.method==='execute'?b.executor:b.owner),blockhash:record.blockhash,lastValidBlockHeight:record.lastValidBlockHeight}).add(...expectedInstructions(sdk,policy,b,record.method,options,record));
 const tx=Transaction.from(Buffer.from(record.signedBytes,'base64'));
 if(!tx.verifySignatures()||!Buffer.from(tx.serializeMessage()).equals(Buffer.from(expected.serializeMessage()))||base58(tx.signature)!==record.signature)throw Error('Saved signed transaction does not match this operation');
 return {tx,intent,binding:b};
}
/** Signing and durable storage are host-injected. No keys enter policy or skill. */
export class PolicyLifecycle{
 constructor(sdk,journal,sign){this.sdk=sdk;this.journal=journal;this.sign=sign;}
 async reconcile(record,policy,owner){
  const {tx}=validateRecord(this.sdk,policy,owner,record);
  await this.sdk.verifyRelease(['revoke','withdraw'].includes(record.method));
  if(record.absence?.kind?.startsWith('expired-'))return record;
  const result=await this.sdk.status(record.signature);
  if(result.status==='uncertain'&&['execute','revoke','tune','deploy','fund','withdraw'].includes(record.method)){
   const height=await this.sdk.connection.getBlockHeight('finalized');
   if(height>record.lastValidBlockHeight){
    const slot=await this.sdk.connection.getSlot('finalized');
    const block=await this.sdk.connection.getBlock(slot,{commitment:'finalized',maxSupportedTransactionVersion:0});
    if(!block||block.blockHeight===null||block.blockHeight<=record.lastValidBlockHeight)return {...record,...result};
    const state=await this.sdk.state(policy,owner,true,slot);
    if(['fund','withdraw'].includes(record.method))return {...record,...result,blockhashExpired:true};
    const unchanged=record.method==='deploy'?state===null:state&&state.revision===record.revision&&(record.method!=='execute'||state.nonce===record.nonce);
    if(unchanged)return {...record,status:'failed',decisionCode:'EXPIRED_UNEXECUTED',absence:{kind:'expired-'+record.method,height:block.blockHeight,slot,nonce:state?.nonce,revision:state?.revision}};
   }
  }
  if(result.status==='settled'){
   const receipt=await this.sdk.connection.getTransaction(record.signature,{commitment:'finalized',maxSupportedTransactionVersion:0});
   if(!receipt)return {...record,status:'uncertain'};
   if(receipt.meta?.err||!receipt.meta||!Buffer.from(receipt.transaction.message.serialize()).equals(Buffer.from(tx.serializeMessage()))||receipt.transaction.signatures[0]!==record.signature)throw Error('Chain receipt does not match saved native transaction');
   if(record.method==='execute'){
    const b=this.sdk.publicBinding(policy,owner),keys=receipt.transaction.message.accountKeys;
    const inner=receipt.meta.innerInstructions?.flatMap(x=>x.instructions)??[];
    const programs=inner.map(i=>keys[i.programIdIndex]?.toBase58());
    if(!programs.includes(b.policy)||!programs.includes('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'))throw Error('Native policy or SPL CPI is missing');
    const amount=units(JSON.parse(record.intent).amount),sourceIndex=keys.findIndex(k=>k.toBase58()===b.tokenAccount),destinationIndex=keys.findIndex(k=>k.toBase58()===JSON.parse(record.intent).recipient);
    const balance=(list,index)=>{const x=list?.find(x=>x.accountIndex===index);if(!x||x.mint!==b.mint)throw Error('Missing token balance proof');return BigInt(x.uiTokenAmount.amount);};
    if(balance(receipt.meta.preTokenBalances,sourceIndex)-balance(receipt.meta.postTokenBalances,sourceIndex)!==amount||balance(receipt.meta.postTokenBalances,destinationIndex)-balance(receipt.meta.preTokenBalances,destinationIndex)!==amount)throw Error('Native transfer balance deltas differ');
   }
  }
  return {...record,...result};
 }
 async submit(policy,owner,method,options={},requestId){
  await validatePolicy(policy);
  const canonical=intentFor(this.sdk,policy,owner,method,options),id=requestId??await digest(encoder.encode(canonical));
  if(!/^[A-Za-z0-9._:-]{8,100}$/.test(id))throw Error('Request ID must be 8–100 ASCII identifier characters');
  return this.journal.locked(async()=>{
   const name='request-'+id,prior=await this.journal.read(name);
   if(prior){
    if(prior.intent!==canonical)throw Error('Request ID conflict; recover the original request');
    const result=await this.reconcile(prior,policy,owner);await this.journal.write(name,result);
    if(['settled','failed'].includes(result.status)){const slot=await this.journal.read('execute-slot');if(slot?.id===id)await this.journal.clear('execute-slot');}
    else if(result.signedBytes&&await this.sdk.connection.getBlockHeight('finalized')<=result.lastValidBlockHeight){await this.sdk.connection.sendRawTransaction(Buffer.from(result.signedBytes,'base64'),{skipPreflight:false,maxRetries:0}).catch(()=>{});}
    return {...result,replayed:true};
   }
   // All commands serialize locally; any unresolved spend blocks another spend.
   if(method==='execute'){
    const slot=await this.journal.read('execute-slot');
    if(slot){const old=await this.journal.read('request-'+slot.id);if(!old)throw Error('Execution journal inconsistency');const reconciled=await this.reconcile(old,policy,owner);await this.journal.write('request-'+slot.id,reconciled);if(!['settled','failed'].includes(reconciled.status))throw Error(`Execution ${slot.id} is uncertain; recover it before a new spend`);await this.journal.clear('execute-slot');}
   }
   const prepared=await this.sdk.prepare(policy,owner,method,options),unsignedMessage=Buffer.from(prepared.transaction.serializeMessage());
   const signed=await this.sign(prepared.transaction,method==='execute'?'executor':'owner');
   if(!Buffer.from(signed.serializeMessage()).equals(unsignedMessage)||!signed.verifySignatures())throw Error('Signer changed the prepared transaction or returned invalid signatures');
   const raw=signed.serialize(),signature=base58(signed.signature);
   const record={id,intent:canonical,method,status:'uncertain',signature,signedBytes:raw.toString('base64'),blockhash:prepared.blockhash,lastValidBlockHeight:prepared.lastValidBlockHeight,nonce:prepared.nonce,revision:prepared.revision,transactionUrl:this.sdk.transactionURL(signature)};
   validateRecord(this.sdk,policy,owner,record);
   // Exact signed proof and unresolved slot are durable before any broadcast.
   await this.journal.write(name,record);if(method==='execute')await this.journal.write('execute-slot',{id});await this.journal.write('last',{id});
   try{const returned=await this.sdk.connection.sendRawTransaction(raw,{skipPreflight:false,maxRetries:0});if(returned!==signature)throw Error('RPC returned a different signature');record.status='submitted';await this.journal.write(name,record);}catch{/* Missing evidence never authorizes a replacement. */}
   return record;
  });
 }
 async recover(id,policy,owner){if(!/^[A-Za-z0-9._:-]{8,100}$/.test(id))throw Error('Invalid request ID');await validatePolicy(policy);return this.journal.locked(async()=>{const prior=await this.journal.read('request-'+id);if(!prior)throw Error('Unknown request ID');const result=await this.reconcile(prior,policy,owner);await this.journal.write('request-'+id,result);return result;});}
}
