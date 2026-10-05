import type {Connection,Transaction} from '@solana/web3.js';
export type Network='solana:testnet'|'solana:devnet';
export interface NativeDeployment {network:Network;sourceBundle:string;policy:string;custody:string;policyData:string;}
export interface Policy {version:1;instance:string;profile:'solana-native-v1';network:Network;id:string;prompt:string;dailyLimit:string;payDiscovery:boolean;sourceBundle:string;policyArtifact:string;rust:string;}
export interface Binding {owner:string;executor:string;mint:string;vault:string;tokenAccount:string;policy:string;policyData:string;custody:string;bump:number;}
export interface VaultState extends Binding {abi:number;sourceBundle:string;policyArtifact:string;vaultId:string;dailyLimit:string;spent:string;spentDay:string;nonce:string;revision:string;approved:boolean;balance:string;}
export interface SDKConfig {network?:Network;rpcUrl?:string;mint?:string;executor?:string;deployment?:NativeDeployment;connection?:Connection;author?:(prompt:string)=>Promise<{dailyLimit:string;payDiscovery?:boolean;unsupported:string[]}>;}
export type PolicyMethod='deploy'|'fund'|'execute'|'revoke'|'withdraw'|'tune';
export interface PolicyOptions {amount?:string;recipient?:string;additionalOwnerOperation?:boolean;}
export interface Operation {id:string;status:'uncertain'|'submitted'|'settled'|'failed';method:PolicyMethod;signature:string;transactionUrl:string;replayed?:boolean;[key:string]:unknown;}
export interface Journal {read(name:string):Promise<any>;write(name:string,value:any):Promise<any>;clear(name:string):Promise<any>;locked<T>(run:()=>Promise<T>):Promise<T>;}
export class NativePolicySDK {constructor(config?:SDKConfig);config:SDKConfig;connection:Connection;generate(prompt:string):Promise<Policy>;checkNetwork():Promise<void>;verifyRelease(recovery?:boolean):Promise<NativeDeployment>;binding(policy:Policy,owner:string,recovery?:boolean):Promise<Binding>;publicBinding(policy:Policy,owner:string):Binding;state(policy:Policy,owner:string,recovery?:boolean,minContextSlot?:number):Promise<VaultState|null>;prepare(policy:Policy,owner:string,method:PolicyMethod,options?:PolicyOptions):Promise<{transaction:Transaction;[key:string]:any}>;instruction(binding:Binding,method:string,fields:any):any;status(signature:string):Promise<any>;transactionURL(signature:string):string;bundle(policy:Policy,owner:string):Promise<any>;skill(policy:Policy,state:VaultState):string;}
export const RELEASE:any;
export const PROFILE:'solana-native-v1';
export const MAX_DAILY_UNITS:bigint;
export function units(value:string):bigint;
export function decimal(value:bigint|string):string;
export function digest(value:Uint8Array):Promise<string>;
export function encodeInstruction(method:string,fields?:any):Uint8Array;
export function decodeState(data:Uint8Array):any;
export function validatePolicy(policy:any):Promise<Policy>;
export function boundedAuthor(prompt:string):Promise<{dailyLimit:string;payDiscovery:boolean;unsupported:string[]}>;

export {Transaction} from '@solana/web3.js';
