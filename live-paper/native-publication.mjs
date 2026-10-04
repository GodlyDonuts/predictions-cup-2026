import fs from 'node:fs';
import path from 'node:path';
import {randomUUID,createHash} from 'node:crypto';
import {readNativePublicationPrefix} from './rust-backend.mjs';
import {createBookPublicationState} from './book-publication-v2.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const need=(ok,code)=>{if(!ok)throw Error('NATIVE_PUBLICATION_'+code);},sha=b=>createHash('sha256').update(b).digest('hex');
export function nativePublicationBinding(runtimeDir,binding){
 need(path.isAbsolute(runtimeDir)&&binding?.schemaVersion===1&&binding.mode==='native_public_book_prefix_v1'&&['binaryHash','engineHash','runtimeLibraryHash'].every(k=>/^[a-f0-9]{64}$/.test(binding[k])),'BINDING');
 const root=path.join(runtimeDir,'native-builds',binding.binaryHash),executable=path.join(root,'baal-backend.exe');
 need(sha(fs.readFileSync(executable))===binding.binaryHash&&sha(fs.readFileSync(path.join(root,'libunwind.dll')))===binding.runtimeLibraryHash,'BINARY_CHANGED');
 return executable;
}
export async function nativePublicationState({runtimeDir,binding,universe,journal,endByte,expectedSha,receiptDir}){
 const executable=nativePublicationBinding(runtimeDir,binding),scopeBytes=Buffer.from(JSON.stringify(universe)+'\n');fs.mkdirSync(receiptDir,{recursive:true,mode:0o700});
 const id=randomUUID(),scope=path.join(receiptDir,id+'.scope.json'),out=path.join(receiptDir,id+'.state.json');fs.writeFileSync(scope,scopeBytes,{flag:'wx',mode:0o600});
 const result=await readNativePublicationPrefix({journal,scope,endByte,expectedSha,out,executable,pinnedBinaryHash:binding.binaryHash});nativePublicationBinding(runtimeDir,binding);
 const trusted=createBookPublicationState(universe);need(result.engineHash===binding.engineHash&&result.scopeHash===trusted.scopeHash&&payloadHash(result.state.scope)===payloadHash(trusted.scope),'ENGINE_OR_SCOPE_CHANGED');
 // Reuse the validated, frozen scope so incremental JS tail validation need not
 // rebuild and hash the entire instrument universe on every new record.
 return{...result,state:{...result.state,scope:trusted.scope},receiptFile:out+'.receipt.json'};
}
