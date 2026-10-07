import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
const sha=b=>createHash('sha256').update(b).digest('hex'),stamp=s=>[s.dev,s.ino,s.size,s.mtimeNs,s.ctimeNs,s.birthtimeNs].map(String).join(':');
// Preliminary synchronous guards check pinned file identity/change metadata.
// The physical write fence MUST request full bytes immediately before fetch.
export function createPinnedCodeGuardV1({workspace,identity,filesystem=fs}){
 const root=path.resolve(workspace),records=Object.entries(identity).map(([relative,hash])=>{
  const file=path.resolve(root,relative);if(!file.startsWith(root+path.sep)||!/^[a-f0-9]{64}$/.test(hash))throw Error('PINNED_CODE_SCOPE');const stat=filesystem.statSync(file,{bigint:true});if(!stat.isFile()||filesystem.lstatSync(file).isSymbolicLink()||sha(filesystem.readFileSync(file))!==hash||stamp(filesystem.statSync(file,{bigint:true}))!==stamp(stat))throw Error('PINNED_CODE_INITIAL');return{file,hash,stamp:stamp(stat)};
 });
 return({full=false}={})=>{for(const r of records){const before=filesystem.statSync(r.file,{bigint:true});if(stamp(before)!==r.stamp)throw Error('PINNED_CODE_CHANGED');if(full&&(sha(filesystem.readFileSync(r.file))!==r.hash||stamp(filesystem.statSync(r.file,{bigint:true}))!==r.stamp))throw Error('PINNED_CODE_BYTES_CHANGED');}return true;};
}
