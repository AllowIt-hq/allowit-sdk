// Build exclusively from committed SDK and CLI sources, never a dirty checkout.
import {execFileSync} from 'node:child_process';
import {mkdirSync,writeFileSync,mkdtempSync,rmSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const cliRoot=resolve(process.env.ALLOWIT_CLI_SOURCE??resolve(root,'../allowit-native-cli'));
const git=(repo,...args)=>execFileSync('git',['-C',repo,...args]);
for(const repo of [root,cliRoot])if(git(repo,'status','--porcelain').toString().trim())throw Error('Commit SDK and CLI before building a release bundle');
const sdkRevision=git(root,'rev-parse','HEAD').toString().trim(),cliRevision=git(cliRoot,'rev-parse','HEAD').toString().trim();
const destination=resolve(process.argv[2]??join(root,'dist-native'));mkdirSync(destination,{recursive:false});
const temporary=mkdtempSync(join(tmpdir(),'allowit-cli-build-'));
try{
 const files=git(cliRoot,'ls-tree','-r','--name-only',cliRevision).toString().trim().split('\n').filter(p=>((p.startsWith('cmd/')||p.startsWith('internal/'))&&p.endsWith('.go'))||['go.mod','go.sum'].includes(p));
 for(const file of files){const path=join(temporary,file);mkdirSync(dirname(path),{recursive:true});writeFileSync(path,git(cliRoot,'show',cliRevision+':'+file));}
 execFileSync('go',['build','-trimpath','-o',destination+'/allowit','./cmd/allowit'],{cwd:temporary,stdio:'inherit'});
 const names=['index.mjs','index.d.ts','release.mjs','token.mjs','lifecycle.mjs','journal.mjs','cli.mjs','browser.mjs','browser.d.ts','package.json','package-lock.json','README.md'],hashes={};
 mkdirSync(destination+'/native-sdk');for(const name of names){const data=git(root,'show',sdkRevision+':native/'+name);writeFileSync(destination+'/native-sdk/'+name,data);hashes[name]=createHash('sha256').update(data).digest('hex');}
 execFileSync('npm',['ci','--omit=dev','--ignore-scripts'],{cwd:destination+'/native-sdk',stdio:'inherit'});
 writeFileSync(destination+'/source.json',JSON.stringify({sdkRevision,cliRevision,files:hashes},null,2)+'\n');
 console.log('Built '+destination+'/allowit with committed SDK '+sdkRevision+' and CLI '+cliRevision);
}finally{rmSync(temporary,{recursive:true,force:true});}
