import {PolicyLifecycle} from './lifecycle.mjs';
export {PolicyLifecycle};
export {NativePolicySDK} from './index.mjs';
/** IndexedDB commit completion is the durability boundary before broadcast. */
export class BrowserJournal{
 constructor(namespace){this.namespace=namespace;}
 async database(){return new Promise((resolve,reject)=>{const r=indexedDB.open('allowit-native-v1',1);r.onupgradeneeded=()=>r.result.createObjectStore('operations');r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});}
 async operation(mode,run){const db=await this.database();try{return await new Promise((resolve,reject)=>{const tx=db.transaction('operations',mode,{durability:'strict'}),store=tx.objectStore('operations');let result;const r=run(store);if(r)r.onsuccess=()=>{result=r.result;};tx.oncomplete=()=>resolve(result??null);tx.onerror=()=>reject(tx.error);tx.onabort=()=>reject(tx.error??Error('Journal commit aborted'));});}finally{db.close();}}
 key(name){return this.namespace+':'+name;}
 entries(prefix='request-'){return this.operation('readonly',s=>s.getAll(IDBKeyRange.bound(this.key(prefix),this.key(prefix)+'\uffff')));}
 read(name){return this.operation('readonly',s=>s.get(this.key(name)));}
 write(name,value){return this.operation('readwrite',s=>s.put(value,this.key(name)));}
 clear(name){return this.operation('readwrite',s=>s.delete(this.key(name)));}
 async locked(run){if(!navigator.locks)throw Error('This browser cannot safely serialize policy operations');return navigator.locks.request('allowit-native:'+this.namespace,{mode:'exclusive'},run);}
}
