import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {nativeExecutable} from '../live-paper/rust-backend.mjs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';

// Preserve the original historical benchmark helper. Current callers must
// independently establish state parity on the exact binary they are archiving.
export function archiveCheckedNativeGraph(runtimeDir,{binaryHash,engineHash,exactStateParity}){
 const binary=fs.readFileSync(nativeExecutable),library=fs.readFileSync(path.join(path.dirname(nativeExecutable),'libunwind.dll'));
 if(sha(binary)!==binaryHash||exactStateParity!==true)throw Error('NATIVE_ARCHIVE_UNCHECKED_BINARY');
 const files=[...fs.readFileSync('backend-rust/build.rs','utf8').matchAll(/^\s*"([^"]+)",$/gm)].map(x=>x[1]);
 if(!files.length||new Set(files).size!==files.length)throw Error('NATIVE_ARCHIVE_SOURCE_LIST');
 const graph=createHash('sha256'),sources={},bytesByName={};
 for(const name of files){
  if(path.isAbsolute(name)||name.includes('..'))throw Error('NATIVE_ARCHIVE_PATH');
  const label=Buffer.from(name),bytes=fs.readFileSync(path.join('backend-rust',name)),labelSize=Buffer.alloc(8),byteSize=Buffer.alloc(8);
  labelSize.writeBigUInt64LE(BigInt(label.length));byteSize.writeBigUInt64LE(BigInt(bytes.length));graph.update(labelSize).update(label).update(byteSize).update(bytes);
  sources[name]=sha(bytes);bytesByName[name]=bytes;
 }
 if(graph.digest('hex')!==engineHash)throw Error('NATIVE_ARCHIVE_SOURCE_GRAPH_CHANGED');
 const immutable=(file,bytes)=>{fs.mkdirSync(path.dirname(file),{recursive:true,mode:0o700});if(fs.existsSync(file)){if(sha(fs.readFileSync(file))!==sha(bytes))throw Error('NATIVE_ARCHIVE_COLLISION');}else fs.writeFileSync(file,bytes,{flag:'wx',mode:0o600});};
 const root=path.join(runtimeDir,'native-builds',binaryHash),sourceRoot=path.join(runtimeDir,'native-sources',engineHash);
 immutable(path.join(root,'baal-backend.exe'),binary);immutable(path.join(root,'libunwind.dll'),library);
 for(const name of files)immutable(path.join(sourceRoot,name),bytesByName[name]);
 immutable(path.join(sourceRoot,'manifest.json'),Buffer.from(JSON.stringify({schemaVersion:1,engineHash,files:sources},null,2)+'\n'));
 return{schemaVersion:1,mode:'native_public_book_prefix_v1',binaryHash,engineHash,runtimeLibraryHash:sha(library)};
}
