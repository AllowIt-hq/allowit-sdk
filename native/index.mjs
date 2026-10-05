import {Buffer} from 'buffer';
import {Connection, PublicKey, SystemProgram, Transaction, TransactionInstruction} from '@solana/web3.js';
import {TOKEN_PROGRAM_ID, getAccount, getMint, getAssociatedTokenAddressSync, createAssociatedTokenAccountIdempotentInstruction} from './token.mjs';
import {RELEASE} from './release.mjs';
export {RELEASE,Transaction};
const refusal=message=>Object.assign(new Error(message),{code:'POLICY_DENIED'});
export const PROFILE = 'solana-native-v1';
export const GENESIS = {'solana:testnet':'4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY','solana:devnet':'EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG'};
export const LOADER = new PublicKey('BPFLoaderUpgradeab1e11111111111111111111111');
export const MAX_DAILY_UNITS = 50_000_000n;
const key = value => new PublicKey(value);
const bytes = value => Buffer.from(value);
const hexBytes = value => {if(!/^[0-9a-f]{64}$/.test(value))throw Error('Invalid digest');return Buffer.from(value,'hex');};
export async function digest(data){return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',data)),b=>b.toString(16).padStart(2,'0')).join('');}
export function units(value){if(typeof value!=='string'||! /^(0|[1-9]\d*)(\.\d{1,6})?$/.test(value))throw Error('Use an exact decimal with at most six places');const [whole,fraction='']=value.split('.');const n=BigInt(whole)*1_000_000n+BigInt(fraction.padEnd(6,'0'));if(n>0xffffffffffffffffn)throw Error('Amount exceeds u64');return n;}
export function decimal(n){n=BigInt(n);const f=(n%1_000_000n).toString().padStart(6,'0').replace(/0+$/,'');return (n/1_000_000n).toString()+(f?'.'+f:'');}
const u64 = value => {const b=Buffer.alloc(8);b.writeBigUInt64LE(BigInt(value));return b;};
export function encodeInstruction(method,fields={}){
 const selectors={initialize:0,deposit:1,approve:2,tune:3,transfer:4,withdraw:5};
 const tag=selectors[method];if(tag===undefined)throw Error('Unsupported native method');
 let tail;
 switch(method){
 case 'initialize':tail=Buffer.concat([hexBytes(fields.vaultId),hexBytes(RELEASE.sourceBundle),hexBytes(RELEASE.artifacts[0].sha256),u64(fields.dailyLimit)]);break;
 case 'approve':tail=Buffer.concat([Buffer.from([fields.approved?1:0]),u64(fields.revision)]);break;
 case 'tune':tail=Buffer.concat([u64(fields.amount),u64(fields.revision)]);break;
 case 'transfer':tail=Buffer.concat([u64(fields.amount),u64(fields.nonce),u64(fields.revision)]);break;
 default:tail=u64(fields.amount);
 }
 return Buffer.concat([Buffer.from([tag]),tail]);
}
export function decodeState(data){
 if(data.length!==320||data[0]!==1||data[298]>1||data.slice(299).some(x=>x!==0))throw Error('Invalid native vault state');
 const names=['owner','executor','mint','tokenAccount','policy'];const state={abi:data[0],bump:data[1]};let offset=2;
 for(const name of names){state[name]=key(data.slice(offset,offset+32)).toBase58();offset+=32;}
 for(const name of ['sourceBundle','policyArtifact','vaultId']){state[name]=bytes(data.slice(offset,offset+32)).toString('hex');offset+=32;}
 for(const name of ['dailyLimit','spent','spentDay','nonce','revision']){state[name]=bytes(data).readBigUInt64LE(offset).toString();offset+=8;}
 state.approved=data[offset]===1;return state;
}
export async function validatePolicy(policy){
 if(!policy||policy.version!==1||policy.profile!==PROFILE||!GENESIS[policy.network])throw Error('Unsupported policy profile/network');
 if(policy.sourceBundle!==RELEASE.sourceBundle||policy.policyArtifact!==RELEASE.artifacts[0].sha256||policy.rust!==RELEASE.sources['policy.rs'])throw Error('Policy source/artifact binding changed');
 if(units(policy.dailyLimit)>MAX_DAILY_UNITS||units(policy.dailyLimit)===0n)throw Error('Daily limit must be positive and within the native ceiling');
 if(typeof policy.instance!=='string'||!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(policy.instance))throw Error('Invalid policy instance');
 if(typeof policy.prompt!=='string'||!policy.prompt.trim()||policy.prompt.length>8000||typeof policy.payDiscovery!=='boolean')throw Error('Invalid policy prompt');
 const identity={instance:policy.instance,profile:policy.profile,network:policy.network,prompt:policy.prompt,dailyLimit:policy.dailyLimit,payDiscovery:policy.payDiscovery,sourceBundle:policy.sourceBundle};
 if(policy.id!==await digest(new TextEncoder().encode(JSON.stringify(identity))))throw Error('Policy identity changed');
 return policy;
}
/** Strict offline author for the bounded native rule. Full prompt authoring is injected by the host. */
export async function boundedAuthor(prompt){
 const match=/^Spend up to (\d+(?:\.\d{1,6})?) (?:tokens|test tokens) per day(?: with PaySH discovery)?\.?$/i.exec(prompt.trim());
 if(!match)throw refusal('This native profile supports a daily token ceiling only. Use “Spend up to 5 test tokens per day” or configure the prompt author. Other rules must not be silently dropped.');
 return {dailyLimit:match[1],payDiscovery:/ with PaySH discovery/i.test(prompt),unsupported:[]};
}
export class NativePolicySDK{
 constructor(config={}){
  this.config={network:'solana:testnet',rpcUrl:'https://api.testnet.solana.com',author:boundedAuthor,...config};
  if(!GENESIS[this.config.network])throw Error('Native lifecycle is restricted to explicit Solana test networks');
  this.connection=config.connection??new Connection(this.config.rpcUrl,'finalized');
 }
 /** The sole generation argument is the owner prompt; network/provider are instance configuration. */
 async generate(prompt){
  if(typeof prompt!=='string'||!prompt.trim()||prompt.length>8000)throw Error('Supply one bounded policy prompt');
  const authored=await this.config.author(prompt);
  if(!authored||!Array.isArray(authored.unsupported)||authored.unsupported.length)throw refusal('This prompt contains constraints the native contract cannot enforce');
  const dailyLimit=decimal(units(authored.dailyLimit));
  const identity={instance:crypto.randomUUID(),profile:PROFILE,network:this.config.network,prompt,dailyLimit,payDiscovery:authored.payDiscovery===true,sourceBundle:RELEASE.sourceBundle};
  const policy={version:1,...identity,id:await digest(new TextEncoder().encode(JSON.stringify(identity))),policyArtifact:RELEASE.artifacts[0].sha256,rust:RELEASE.sources['policy.rs']};
  await validatePolicy(policy);return policy;
 }
 async checkNetwork(){if(await this.connection.getGenesisHash()!==GENESIS[this.config.network])throw Error('RPC genesis does not match the policy network');}
 async verifyRelease(recovery=false){
  await this.checkNetwork();const d=this.config.deployment;
  if(!d||d.network!==this.config.network||d.sourceBundle!==RELEASE.sourceBundle)throw Error('Configure an independently verified native deployment');
  for(const [program,expected] of (recovery?[[d.custody,RELEASE.artifacts[1].sha256]]:[[d.policy,RELEASE.artifacts[0].sha256],[d.custody,RELEASE.artifacts[1].sha256]])){
   const a=await this.connection.getAccountInfo(key(program),'finalized');
   if(!a?.executable||!a.owner.equals(LOADER)||a.data.length!==36||a.data.readUInt32LE(0)!==2)throw Error('Invalid deployed native program');
   const linked=key(a.data.subarray(4));const [canonical]=PublicKey.findProgramAddressSync([key(program).toBuffer()],LOADER);
   if(!linked.equals(canonical))throw Error('Invalid ProgramData link');
   const pd=await this.connection.getAccountInfo(linked,'finalized');
   if(!pd?.owner.equals(LOADER)||pd.data.length<46||pd.data.readUInt32LE(0)!==3||pd.data[12]!==0||await digest(pd.data.subarray(45))!==expected)throw Error('Native deployment must have exact immutable artifact bytes');
  }
  if(d.policyData!==PublicKey.findProgramAddressSync([key(d.policy).toBuffer()],LOADER)[0].toBase58())throw Error('Configured policy ProgramData mismatch');
  const mint=await getMint(this.connection,key(this.config.mint),'finalized',TOKEN_PROGRAM_ID);
  if(mint.decimals!==6||!mint.isInitialized)throw Error('Native profile needs a six-decimal classic SPL mint');
  return d;
 }
 async binding(policy,owner,recovery=false){
  await validatePolicy(policy);if(policy.network!==this.config.network)throw Error('Policy network mismatch');
  const d=await this.verifyRelease(recovery);return this.publicBinding(policy,owner,d);
 }
 publicBinding(policy,owner,d=this.config.deployment){
  if(!d||policy.network!==this.config.network||d.network!==this.config.network||d.sourceBundle!==RELEASE.sourceBundle)throw Error('Deployment/network mismatch');
  if(d.policyData!==PublicKey.findProgramAddressSync([key(d.policy).toBuffer()],LOADER)[0].toBase58())throw Error('Policy ProgramData mismatch');
  const [vault,bump]=PublicKey.findProgramAddressSync([Buffer.from('allowit-vault-v1'),key(owner).toBuffer(),hexBytes(policy.id)],key(d.custody));
  const tokenAccount=getAssociatedTokenAddressSync(key(this.config.mint),vault,true,TOKEN_PROGRAM_ID);
  return {owner:key(owner).toBase58(),executor:key(this.config.executor).toBase58(),mint:key(this.config.mint).toBase58(),vault:vault.toBase58(),tokenAccount:tokenAccount.toBase58(),policy:d.policy,policyData:d.policyData,custody:d.custody,bump};
 }
 async state(policy,owner,recovery=false,minContextSlot){
  const b=await this.binding(policy,owner,recovery);let info;
  if(minContextSlot!==undefined){const result=await this.connection.getAccountInfoAndContext(key(b.vault),{commitment:'finalized',minContextSlot});if(result.context.slot<minContextSlot)throw Error('Incoherent finalized vault observation');info=result.value;}else info=await this.connection.getAccountInfo(key(b.vault),'finalized');
  if(!info)return null;if(!info.owner.equals(key(b.custody)))throw Error('Wrong vault account owner');
  const s=decodeState(info.data);
  for(const name of ['owner','executor','mint','tokenAccount','policy','bump'])if(s[name]!==b[name])throw Error(`Vault ${name} mismatch`);
  if(s.vaultId!==policy.id||s.sourceBundle!==policy.sourceBundle||s.policyArtifact!==policy.policyArtifact||BigInt(s.dailyLimit)>MAX_DAILY_UNITS)throw Error('Vault identity/limits mismatch');
  const tokens=await getAccount(this.connection,key(b.tokenAccount),'finalized',TOKEN_PROGRAM_ID);
  if(!tokens.owner.equals(key(b.vault))||!tokens.mint.equals(key(b.mint))||tokens.delegate||tokens.closeAuthority)throw Error('Unsafe custody token account');
  return {...s,...b,balance:tokens.amount.toString()};
 }
 instruction(b,method,fields){
  const meta=(name,writable=false,signer=false)=>({pubkey:key(b[name]??name),isWritable:writable,isSigner:signer});let accounts;
  switch(method){
  case 'initialize':accounts=[meta('vault',true),meta('owner',true,true),meta('mint'),meta('tokenAccount'),meta('policy'),meta('executor'),meta(SystemProgram.programId.toBase58()),meta('policyData')];break;
  case 'approve':case 'tune':accounts=[meta('vault',true),meta('owner',false,true),meta('policy'),...(method==='approve'&&!fields.approved?[]:[meta('policyData')])];break;
  case 'deposit':accounts=[meta('vault',true),meta('owner',false,true),meta(fields.source,true),meta('tokenAccount',true),meta('mint'),meta(TOKEN_PROGRAM_ID.toBase58())];break;
  case 'transfer':accounts=[meta('vault',true),meta('executor',false,true),meta('tokenAccount',true),meta(fields.destination,true),meta('mint'),meta(TOKEN_PROGRAM_ID.toBase58()),meta('policy'),meta('policyData')];break;
  case 'withdraw':accounts=[meta('vault',true),meta('owner',false,true),meta('tokenAccount',true),meta(fields.destination,true),meta('mint'),meta(TOKEN_PROGRAM_ID.toBase58())];break;
  default:throw Error('Unsupported native method');
  }
  return new TransactionInstruction({programId:key(b.custody),keys:accounts,data:encodeInstruction(method,fields)});
 }
 async prepare(policy,owner,method,options={}){
  const recovery=['revoke','withdraw'].includes(method);const b=await this.binding(policy,owner,recovery);const s=await this.state(policy,owner,recovery);const amount=options.amount===undefined?undefined:units(options.amount);
  if(['fund','execute','withdraw','tune'].includes(method)&&amount===undefined)throw Error('Specify the amount');
  if(amount===0n&&method!=='tune')throw refusal('Amount must be positive');let instructions=[];let signer=b.owner;
  if(method==='deploy'){
   if(s)throw Error('Vault already exists; recover its status instead of redeploying');
   instructions=[createAssociatedTokenAccountIdempotentInstruction(key(owner),key(b.tokenAccount),key(b.vault),key(b.mint),TOKEN_PROGRAM_ID),this.instruction(b,'initialize',{vaultId:policy.id,dailyLimit:units(policy.dailyLimit)}),this.instruction(b,'approve',{approved:true,revision:0n})];
  }else{
   if(!s)throw Error('Deploy the native policy first');
   switch(method){
   case 'fund':{const source=getAssociatedTokenAddressSync(key(b.mint),key(owner));const a=await getAccount(this.connection,source,'finalized');if(!a.owner.equals(key(owner))||a.amount<amount)throw refusal('Insufficient owner test-token balance');instructions=[this.instruction(b,'deposit',{amount,source:source.toBase58()})];break;}
   case 'execute':{
    if(!s.approved)throw refusal('Standing approval is withdrawn');const day=BigInt(Math.floor((await this.chainTime())/86400));const spent=day===BigInt(s.spentDay)?BigInt(s.spent):0n;
    if(amount>BigInt(s.dailyLimit)-spent)throw refusal('Daily limit exceeded');if(amount>BigInt(s.balance))throw refusal('Insufficient vault balance');
    const destination=await getAccount(this.connection,key(options.recipient),'finalized',TOKEN_PROGRAM_ID);
    if(!destination.mint.equals(key(b.mint))||destination.owner.equals(key(b.vault)))throw Error('Invalid destination token account');
    signer=b.executor;instructions=[this.instruction(b,'transfer',{amount,destination:options.recipient,nonce:s.nonce,revision:s.revision})];break;
   }
   case 'revoke':instructions=[this.instruction(b,'approve',{approved:false,revision:s.revision})];break;
   case 'tune':if(amount>MAX_DAILY_UNITS)throw refusal('Daily limit exceeds compiled ceiling');instructions=[this.instruction(b,'tune',{amount,revision:s.revision})];break;
   case 'withdraw':{const destination=getAssociatedTokenAddressSync(key(b.mint),key(owner));instructions=[createAssociatedTokenAccountIdempotentInstruction(key(owner),destination,key(owner),key(b.mint)),this.instruction(b,'withdraw',{amount,destination:destination.toBase58()})];break;}
   default:throw Error('Unsupported policy command');
   }
  }
  const latest=await this.connection.getLatestBlockhash('finalized');const tx=new Transaction({...latest,feePayer:key(signer)}).add(...instructions);
  return {transaction:tx,method,binding:b,policy,owner,options,nonce:s?.nonce,revision:s?.revision,lastValidBlockHeight:latest.lastValidBlockHeight,blockhash:latest.blockhash,amount:amount?.toString(),recipient:options.recipient};
 }
 async bundle(policy,owner){await validatePolicy(policy);this.publicBinding(policy,owner);return {version:1,policy,context:{owner,network:policy.network,mint:this.config.mint,executor:this.config.executor,deployment:this.config.deployment}};}
 async chainTime(){const slot=await this.connection.getSlot('finalized');const time=await this.connection.getBlockTime(slot);if(time===null)throw Error('Chain time is unavailable');return time;}
 async status(signature){
  await this.checkNetwork();const value=(await this.connection.getSignatureStatuses([signature],{searchTransactionHistory:true})).value[0];
  if(!value)return {status:'uncertain',signature,transactionUrl:this.transactionURL(signature)};
  if(value.err)return {status:value.confirmationStatus==='finalized'?'failed':'uncertain',signature,error:value.err,transactionUrl:this.transactionURL(signature)};
  return {status:value.confirmationStatus==='finalized'?'settled':'submitted',signature,transactionUrl:this.transactionURL(signature)};
 }
 transactionURL(signature){if(!/^[1-9A-HJ-NP-Za-km-z]{80,90}$/.test(signature))throw Error('Invalid transaction signature');return `https://explorer.solana.com/tx/${signature}?cluster=${this.config.network.split(':')[1]}`;}
 skill(policy,state){
  if(!state?.approved||state.sourceBundle!==policy.sourceBundle||state.policyArtifact!==policy.policyArtifact)throw Error('Finalize deployment and standing approval before issuing a skill');
  const lines=['---',`name: allowit-policy-${policy.id.slice(0,16)}`,'description: Execute transfers governed by this AllowIt native Solana policy.','---','',`Policy ${policy.id}; ${policy.network}; mint ${state.mint}.`,`Owner ${state.owner}; executor ${state.executor}; vault ${state.vault}; custody ${state.custody}.`,`Enforced: standing approval and up to ${decimal(state.dailyLimit)} test tokens per UTC day.`,`Task: ${JSON.stringify(policy.prompt)}. Purpose and recipient restrictions are not enforced by this native artifact.`,'','Import the issued executor.json with `allowit policy import executor.json` on the executor device. Configure only the designated ALLOWIT_EXECUTOR_KEYPAIR there; never provide the owner secret key. Run `allowit policy status` before acting. For each new operation, choose one ALLOWIT_REQUEST_ID and run `allowit policy execute RECIPIENT_TOKEN_ACCOUNT AMOUNT`. Keep that ID and exact intent for retries. Exit 6 means replay of the earlier receipt, not another payment. The configured executor signs; this skill contains no keys. After an uncertain result, recover the saved operation with `allowit policy status`; never submit a replacement. A settled transfer does not prove delivery of a purchased service.'];
  if(policy.payDiscovery)lines.push('','For paid APIs, discover providers through PaySH: https://pay.sh/docs/using-pay/skills. Payment execution through PaySH is unavailable in this profile. Do not pay using another wallet or Pay payment tools as a fallback. Report unsupported payment execution.');
  return lines.join('\n')+'\n';
 }
}
