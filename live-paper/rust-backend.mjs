import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {validateConfig} from './model.mjs';

export const nativeExecutable=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../backend-rust/target/release',process.platform==='win32'?'baal-backend.exe':'baal-backend');
// The native backend consumes an explicit original-byte prefix. It cannot read
// credentials, place orders, rewrite source records, or refit the supplied bank.
export async function replayNativeBank({journal,bank,endByte,expectedSha,out,workers=8,executable=nativeExecutable}){
 validateBinding({endByte,expectedSha,out,workers});
 const bankFileHash=createHash('sha256').update(fs.readFileSync(bank)).digest('hex');
 const args=['replay','--journal',journal,'--bank',bank,'--end-byte',String(endByte),'--expected-sha',expectedSha,'--workers',String(workers),'--out',out];
 await invoke(executable,args);
 const result=JSON.parse(fs.readFileSync(out,'utf8'));
 checkResult(result,expectedSha,endByte,result.replay);
 if(!result.replay.bankSourceBinding||result.bankFileHash!==bankFileHash)throw Error('Native result binding mismatch');
 receipt({out,expectedSha,endByte,executable,result,bankFileHash});
 return result;
}
export async function sweepNativeCapture({journal,config,endByte,expectedSha,out,workers=8,executable=nativeExecutable}){
 validateBinding({endByte,expectedSha,out,workers});
 const c=JSON.parse(fs.readFileSync(config,'utf8'));validateConfig(c);if(c.feeBps!==0)throw Error('Dip sweep requires zero explicit fees');
 const configFileHash=createHash('sha256').update(fs.readFileSync(config)).digest('hex');
 await invoke(executable,['sweep','--journal',journal,'--config',config,'--end-byte',String(endByte),'--expected-sha',expectedSha,'--workers',String(workers),'--out',out]);
 const result=JSON.parse(fs.readFileSync(out,'utf8'));checkResult(result,expectedSha,endByte,result.sweep);if(result.configFileHash!==configFileHash)throw Error('Native config binding mismatch');
 // Derived immutable receipt links the native source graph and exact config.
 receipt({out,expectedSha,endByte,executable,result,configFileHash});
 return result;
}
export async function evaluateNativeCapture({journal,declaration,endByte,expectedSha,out,workers=8,executable=nativeExecutable}){
 validateBinding({endByte,expectedSha,out,workers});
 const declarationFileHash=createHash('sha256').update(fs.readFileSync(declaration)).digest('hex');
 await invoke(executable,['evaluate','--journal',journal,'--declaration',declaration,'--end-byte',String(endByte),'--expected-sha',expectedSha,'--workers',String(workers),'--out',out]);
 const result=JSON.parse(fs.readFileSync(out,'utf8'));checkResult(result,expectedSha,endByte,result.evaluation);
 if(result.declarationFileHash!==declarationFileHash)throw Error('Native declaration binding mismatch');
 receipt({out,expectedSha,endByte,executable,result,declarationFileHash});return result;
}
function validateBinding({endByte,expectedSha,out,workers}){
 if(!Number.isSafeInteger(endByte)||endByte<1||!/^[a-f0-9]{64}$/.test(expectedSha)||!Number.isSafeInteger(workers)||workers<1||workers>32)throw Error('Invalid native replay binding');
 if(fs.existsSync(out)||fs.existsSync(out+'.receipt.json'))throw Error('Refusing to overwrite a native replay');
}
function checkResult(result,sha,bytes,body){
 if(result.capture.source_hash!==sha||result.capture.source_bytes!==bytes||!/^[a-f0-9]{64}$/.test(result.capture.engine_hash)||body?.competitionOrders!==0||body?.networkRequests!==0)throw Error('Native result binding mismatch');
}
function receipt({out,expectedSha,endByte,executable,result,bankFileHash,configFileHash,declarationFileHash}){
 fs.writeFileSync(out+'.receipt.json',JSON.stringify({at:new Date().toISOString(),sourceHash:expectedSha,sourceBytes:endByte,engineHash:result.capture.engine_hash,bankFileHash,configFileHash,declarationFileHash,binaryHash:createHash('sha256').update(fs.readFileSync(executable)).digest('hex'),outputHash:createHash('sha256').update(fs.readFileSync(out)).digest('hex'),competitionOrders:0},null,2)+'\n',{flag:'wx',mode:0o600});
}
async function invoke(executable,args){
 await new Promise((resolve,reject)=>{
  const child=spawn(executable,args,{windowsHide:true,stdio:['ignore','ignore','pipe']});let error='',done=false;
  child.stderr.on('data',chunk=>{error+=chunk.toString();if(error.length>4096)child.kill();});
  child.once('error',()=>{done=true;reject(Error('Native replay binary unavailable; run backend-rust/build.ps1'));});
  child.once('close',code=>{if(done)return;if(code!==0){let label='NATIVE_REPLAY_FAILED';try{const v=JSON.parse(error);if(/^[A-Z_]+$/.test(v.error))label=v.error;}catch{}reject(Error(label));}else resolve();});
 });
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const options=new Map(),allowed=new Set(['--mode','--journal','--bank','--config','--declaration','--end-byte','--expected-sha','--out','--workers']);
 for(let i=2;i<process.argv.length;i+=2){const [k,v]=process.argv.slice(i,i+2);if(!allowed.has(k)||!v||v.startsWith('--')||options.has(k))throw Error('Invalid native replay arguments');options.set(k,v);}
 const mode=options.get('--mode')||'replay';if(!['replay','sweep','evaluate'].includes(mode))throw Error('Unknown native mode');
 for(const k of ['--journal',mode==='replay'?'--bank':mode==='sweep'?'--config':'--declaration','--end-byte','--expected-sha','--out'])if(!options.has(k))throw Error('Required '+k);
 const args={journal:options.get('--journal'),bank:options.get('--bank'),config:options.get('--config'),declaration:options.get('--declaration'),endByte:Number(options.get('--end-byte')),expectedSha:options.get('--expected-sha'),out:options.get('--out'),workers:Number(options.get('--workers')||8)};
 const r=await (mode==='replay'?replayNativeBank:mode==='sweep'?sweepNativeCapture:evaluateNativeCapture)(args),body=r.replay||r.sweep||r.evaluation;
 console.log(JSON.stringify({sourceVerified:true,engineHash:r.capture.engine_hash,sourceRecords:r.capture.source_records,sourceBytes:r.capture.source_bytes,candidates:body.candidates,captureMs:r.capture.verify_ms,prepareMs:body.prepareMs,replayMs:body.replayMs,competitionOrders:0,profitabilityEstablished:false}));
}
