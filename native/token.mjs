import {Buffer} from 'buffer';
import {PublicKey, SystemProgram, TransactionInstruction} from '@solana/web3.js';
export const TOKEN_PROGRAM_ID=new PublicKey('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA');
const ATA_PROGRAM=new PublicKey('ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL');
export function getAssociatedTokenAddressSync(mint,owner,offCurve=false){
 if(!offCurve&&!PublicKey.isOnCurve(owner.toBytes()))throw Error('Owner must be an Ed25519 account');
 return PublicKey.findProgramAddressSync([owner.toBuffer(),TOKEN_PROGRAM_ID.toBuffer(),mint.toBuffer()],ATA_PROGRAM)[0];
}
export function createAssociatedTokenAccountIdempotentInstruction(payer,account,owner,mint){
 return new TransactionInstruction({programId:ATA_PROGRAM,data:Buffer.from([1]),keys:[
  {pubkey:payer,isSigner:true,isWritable:true},{pubkey:account,isSigner:false,isWritable:true},
  ...[owner,mint,SystemProgram.programId,TOKEN_PROGRAM_ID].map(pubkey=>({pubkey,isSigner:false,isWritable:false}))]});
}
async function tokenData(connection,address,size){const a=await connection.getAccountInfo(address,'finalized');if(!a||!a.owner.equals(TOKEN_PROGRAM_ID)||a.data.length!==size)throw Error('Expected a classic SPL Token account');return a.data;}
export async function getMint(connection,address){const d=await tokenData(connection,address,82);if(d[44]!==6||d[45]!==1||d.readUInt32LE(46)!==0)throw Error('Expected an initialized six-decimal test mint');return {decimals:d[44],isInitialized:true};}
export async function getAccount(connection,address){
 const d=await tokenData(connection,address,165);if(d[108]!==1)throw Error('Expected an initialized unfrozen token account');
 for(const offset of [72,109,129])if(d.readUInt32LE(offset)>1)throw Error('Invalid SPL option');
 return {mint:new PublicKey(d.subarray(0,32)),owner:new PublicKey(d.subarray(32,64)),amount:d.readBigUInt64LE(64),delegate:d.readUInt32LE(72)?new PublicKey(d.subarray(76,108)):null,closeAuthority:d.readUInt32LE(129)?new PublicKey(d.subarray(133,165)):null};
}
