import type {NativePolicySDK,Policy,PolicyMethod,PolicyOptions,Operation,Journal} from './index.mjs';
import type {Transaction} from '@solana/web3.js';
export class PolicyLifecycle {constructor(sdk:NativePolicySDK,journal:Journal,sign:(transaction:Transaction,role:'owner'|'executor')=>Promise<Transaction>);submit(policy:Policy,owner:string,method:PolicyMethod,options?:PolicyOptions,requestId?:string):Promise<Operation>;recover(id:string,policy:Policy,owner:string):Promise<Operation>;}
export class BrowserJournal implements Journal {constructor(namespace:string);entries(prefix?:string):Promise<any[]>;read(name:string):Promise<any>;write(name:string,value:any):Promise<any>;clear(name:string):Promise<any>;locked<T>(run:()=>Promise<T>):Promise<T>;}

export {NativePolicySDK} from './index.mjs';
