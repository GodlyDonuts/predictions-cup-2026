import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {nativeExecutable} from '../live-paper/rust-backend.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex');
export function archiveNativePublication(runtimeDir,{bindingEvidence=JSON.parse(fs.readFileSync('data/baal-native-publication-benchmark-20261004.json'))}={}){
 const benchmark=bindingEvidence,binary=fs.readFileSync(nativeExecutable),binaryHash=sha(binary),library=fs.readFileSync(path.join(path.dirname(nativeExecutable),'libunwind.dll'));
 if(binaryHash!==benchmark.binaryHash||!benchmark.exactStateParity)throw Error('Benchmarked native binary changed');
 const files=['Cargo.toml','Cargo.lock','build.rs','.cargo/config.toml','src/main.rs','src/money.rs','src/types.rs','src/capture.rs','src/engine.rs','src/replay.rs','src/trace.rs','src/sweep.rs','src/evaluation.rs','src/publication.rs'];
 const hash=createHash('sha256'),sources={};for(const name of files){const p=Buffer.from(name),bytes=fs.readFileSync(path.join('backend-rust',name)),len=Buffer.alloc(8);len.writeBigUInt64LE(BigInt(p.length));hash.update(len).update(p);const size=Buffer.alloc(8);size.writeBigUInt64LE(BigInt(bytes.length));hash.update(size).update(bytes);sources[name]=sha(bytes);}
 const engineHash=hash.digest('hex');if(engineHash!==benchmark.engineHash)throw Error('Benchmarked native source graph changed');
 const root=path.join(runtimeDir,'native-builds',binaryHash),sourceRoot=path.join(runtimeDir,'native-sources',engineHash);
 const immutable=(file,bytes)=>{fs.mkdirSync(path.dirname(file),{recursive:true,mode:0o700});if(fs.existsSync(file)){if(sha(fs.readFileSync(file))!==sha(bytes))throw Error('Native archive collision');}else fs.writeFileSync(file,bytes,{flag:'wx',mode:0o600});};
 immutable(path.join(root,'baal-backend.exe'),binary);immutable(path.join(root,'libunwind.dll'),library);for(const name of files)immutable(path.join(sourceRoot,name),fs.readFileSync(path.join('backend-rust',name)));
 immutable(path.join(sourceRoot,'manifest.json'),Buffer.from(JSON.stringify({schemaVersion:1,engineHash,files:sources},null,2)+'\n'));
 return{schemaVersion:1,mode:'native_public_book_prefix_v1',binaryHash,engineHash,runtimeLibraryHash:sha(library)};
}
