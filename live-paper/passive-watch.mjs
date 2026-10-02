import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {createPassiveCapture,consumePassiveCapture,passiveCaptureReport} from './passive-capture.mjs';
import {atomicJson,lock} from './runtime.mjs';

const hash=value=>createHash('sha256').update(typeof value==='string'||Buffer.isBuffer(value)?value:JSON.stringify(value)).digest('hex');
const codeFiles=['passive-watch.mjs','passive-capture.mjs','passive-observer.mjs','../paper/fixed.mjs','../feed/contract.mjs'];
export function passiveCodeIdentity(root=path.dirname(fileURLToPath(import.meta.url))){return Object.fromEntries(codeFiles.map(p=>[p,hash(fs.readFileSync(path.resolve(root,p)))]));}
function sourceFile(runtime,name){if(typeof name!=='string'||path.basename(name)!==name||!/^realtime-[A-Za-z0-9._-]+\.jsonl$/.test(name))throw Error('Invalid market capture filename');return path.join(runtime,name);}
function completeLines(bytes){const end=bytes.lastIndexOf(10);return end<0?[]:bytes.subarray(0,end+1).toString('utf8').split('\n').slice(0,-1);}
function append(file,row){const fd=fs.openSync(file,'a',0o600),bytes=Buffer.from(JSON.stringify(row)+'\n');try{let written=0;while(written<bytes.length){const n=fs.writeSync(fd,bytes,written,bytes.length-written);if(n<=0)throw Error('Passive journal short write');written+=n;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}}

export function openPassiveWatch(runtime,{initialize=false,now=Date.now,codeIdentity=passiveCodeIdentity()}={}){
  const bankFile=path.join(runtime,'passive-bank.json'),journalFile=path.join(runtime,'passive-journal.jsonl');
  let bank;
  if(initialize){
    if(fs.existsSync(bankFile)||fs.existsSync(journalFile))throw Error('Refusing to replace an existing passive cohort');
    const session=JSON.parse(fs.readFileSync(path.join(runtime,'realtime-session.json'))),capturePath=sourceFile(runtime,session.file);
    const source=fs.readFileSync(capturePath),lines=completeLines(source),prefix=Buffer.from(lines.length?lines.join('\n')+'\n':'');
    const cutoff=new Date(now()).toISOString(),sourcePrefixHash=hash(prefix),sessionId=hash(session.file).slice(0,32);
    bank={schemaVersion:1,mode:'measurement_only',cutoff,sessionId,sourceFile:session.file,scope:session.scope,
      sourcePrefixBytes:prefix.length,sourcePrefixHash,codeIdentity,sourceHash:hash({sourcePrefixHash,cutoff,sessionId,scope:session.scope,codeIdentity})};
    fs.writeFileSync(bankFile,JSON.stringify(bank,null,2),{flag:'wx',mode:0o600});
    fs.closeSync(fs.openSync(journalFile,'wx',0o600));
  }else bank=JSON.parse(fs.readFileSync(bankFile));
  if(bank.schemaVersion!==1||bank.mode!=='measurement_only'||hash(bank.codeIdentity)!==hash(codeIdentity)
    ||bank.sourceHash!==hash({sourcePrefixHash:bank.sourcePrefixHash,cutoff:bank.cutoff,sessionId:bank.sessionId,scope:bank.scope,codeIdentity:bank.codeIdentity}))throw Error('Passive source identity changed');
  const capturePath=sourceFile(runtime,bank.sourceFile),initialSource=fs.readFileSync(capturePath);
  if(initialSource.length<bank.sourcePrefixBytes||hash(initialSource.subarray(0,bank.sourcePrefixBytes))!==bank.sourcePrefixHash)throw Error('Passive source prefix changed');
  let state=createPassiveCapture({...bank,sourceHash:bank.sourceHash}),offset=0,sequence=0,previous=hash(bank);
  const journal=fs.readFileSync(journalFile,'utf8');if(journal&&!journal.endsWith('\n'))throw Error('Incomplete passive journal');
  for(const line of journal.split('\n').filter(Boolean)){
    const row=JSON.parse(line),{hash:recordHash,...body}=row;
    if(recordHash!==hash(body)||body.sequence!==++sequence||body.previous!==previous)throw Error('Passive journal integrity failure');
    const end=initialSource.indexOf(10,offset);if(end<0)throw Error('Source capture truncated');
    const raw=initialSource.subarray(offset,end+1);if(hash(raw)!==body.sourceLineHash)throw Error('Processed source capture changed');
    const output=consumePassiveCapture(state,JSON.parse(raw.toString('utf8')));
    if(hash(output.records)!==body.recordsHash||hash(output.events)!==body.eventsHash||hash(output.state)!==body.stateHash)throw Error('Passive replay mismatch');
    state=output.state;offset=end+1;previous=recordHash;
  }
  const consume=()=>{
    const stat=fs.statSync(capturePath);if(stat.size<offset)throw Error('Source capture truncated');
    const size=Math.min(stat.size-offset,8*1024*1024);if(size===0)return 0;
    const fd=fs.openSync(capturePath,'r'),bytes=Buffer.alloc(size);let count;
    try{count=fs.readSync(fd,bytes,0,size,offset);}finally{fs.closeSync(fd);}
    const lines=completeLines(bytes.subarray(0,count));if(!lines.length&&count===8*1024*1024)throw Error('Oversized passive source row');let consumed=0;
    for(const line of lines){
      const raw=Buffer.from(line+'\n'),output=consumePassiveCapture(state,JSON.parse(line));
      const body={sequence:sequence+1,previous,sourceLineHash:hash(raw),recordsHash:hash(output.records),eventsHash:hash(output.events),stateHash:hash(output.state),
        captureIndex:output.state.captureIndex,records:output.records,events:output.events};
      const recordHash=hash(body);append(journalFile,{...body,hash:recordHash});state=output.state;offset+=raw.length;sequence++;previous=recordHash;consumed++;
    }
    return consumed;
  };
  const status=()=>{const report=passiveCaptureReport(state),reportedAt=new Date(now()).toISOString();return {...report,
    markets:report.markets.map(m=>({...m,ready:m.ready&&m.updatedAt!==null&&now()-Date.parse(m.updatedAt)>=0&&now()-Date.parse(m.updatedAt)<20000})),
    sourceFile:bank.sourceFile,sourceBytes:offset,journalSequence:sequence,journalHash:previous,
    sourceAgeMs:Math.max(0,now()-fs.statSync(capturePath).mtimeMs),codeIdentity:bank.codeIdentity,reportedAt};};
  const sessionCurrent=()=>JSON.parse(fs.readFileSync(path.join(runtime,'realtime-session.json'))).file===bank.sourceFile;
  return Object.freeze({consume,status,sessionCurrent});
}

async function main(){
  const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url))),runtime=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
  const stopped=()=>[path.join(workspace,'STOP'),path.join(runtime,'STOP'),path.join(runtime,'PASSIVE_STOP')].some(p=>fs.existsSync(p));
  if(stopped())throw Error('Passive STOP exists');
  const release=lock(path.join(runtime,'passive-watch.lock'));let control=false,watch;
  process.on('SIGINT',()=>control=true);process.on('SIGTERM',()=>control=true);
  try{
    watch=openPassiveWatch(runtime,{initialize:process.argv.includes('--init')});
    while(!control&&!stopped()){
      watch.consume();const report=watch.status(),current=watch.sessionCurrent()&&!report.sourceEnded;
      atomicJson(path.join(runtime,'passive-stats.json'),{...report,runtime:{status:current?report.sourceAgeMs<20000?'running':'waiting_for_updates':'source_session_ended',pid:process.pid}});
      if(!current||process.argv.includes('--once'))break;
      await new Promise(resolve=>setTimeout(resolve,2000));
    }
    const report=watch.status();atomicJson(path.join(runtime,'passive-stats.json'),{...report,runtime:{status:process.argv.includes('--once')?'initialized':watch.sessionCurrent()&&!report.sourceEnded?'stopped':'source_session_ended',pid:process.pid}});
  }catch{
    const at=new Date().toISOString();let report={mode:'passive_opportunity_measurement',ordersSubmitted:0,simulatedFills:null,pnl:null,markets:[]};
    try{report=watch.status();}catch{}
    const failure={...report,reportedAt:at,markets:report.markets.map(m=>({...m,ready:false})),runtime:{status:'failed',pid:process.pid,reason:'passive_worker_failed_closed'}};
    try{atomicJson(path.join(runtime,'passive-stats.json'),failure);}catch{}
    try{atomicJson(path.join(runtime,'passive-error.json'),{at,reason:'passive_worker_failed_closed',pid:process.pid});}catch{}
    process.exitCode=1;
  }
  finally{release();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))await main();
