import fs from 'node:fs';import {TapeCheckpointDeltaV1} from './tape-checkpoint-delta-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('TAPE_COMPACT_'+c);};
// Caller must first verify the outer journal chain (ordinary Journal or native
// restore). This bounded tail pass reconstructs logical state, not provenance.
export function restoreTapeLogicalStateV1(journal,{maximumBytes=536870912}={}){
 if(!journal.last)return null;
 if(journal.last.state?.codec!=='tape_market_delta_v1')return journal.last.state;
 const fd=fs.openSync(journal.file,'r'),stamp=fs.fstatSync(fd),rows=[];let position=stamp.size,carry=Buffer.alloc(0),scanned=0,expectedSequence=journal.sequence,expectedHash=journal.previous,seed=null;
 try{
  const last=Buffer.alloc(1);fs.readSync(fd,last,0,1,stamp.size-1);need(last[0]===10,'PARTIAL_SOURCE');
  while(position>0&&!seed){const length=Math.min(position,1048576);position-=length;scanned+=length;need(scanned<=maximumBytes,'RESTORE_BOUND');const chunk=Buffer.alloc(length);need(fs.readSync(fd,chunk,0,length,position)===length,'SOURCE_TRUNCATED');const data=Buffer.concat([chunk,carry]);let end=data.length;carry=Buffer.alloc(0);
   while(end>0&&!seed){if(data[end-1]===10)end--;if(!end)break;const prior=data.lastIndexOf(10,end-1);if(prior<0&&position>0){carry=data.subarray(0,end);break;}const row=JSON.parse(data.subarray(prior+1,end).toString()),{hash,...body}=row;
    need(body.sequence===expectedSequence&&hash===expectedHash&&journal.hash(body)===hash,'TAIL_CHAIN');expectedSequence--;expectedHash=body.previous;
    if(body.state?.codec==='tape_market_delta_v1'){need(rows.length<63&&body.state.sequence===body.sequence,'CHECKPOINT_DISTANCE');rows.push(body.state);}else{need(body.state?.schemaVersion===1&&body.state.seenIdentityVersion===2,'LEGACY_CHECKPOINT');seed=body;}
    end=prior+1;
   }
  }
  need(seed,'CHECKPOINT_MISSING');const decoder=new TapeCheckpointDeltaV1(seed.state,seed.sequence);for(const frame of rows.reverse())decoder.apply(frame);need(decoder.sequence===journal.sequence,'RESTORE_SEQUENCE');
  const current=fs.statSync(journal.file);need(current.dev===stamp.dev&&current.ino===stamp.ino&&current.size===stamp.size,'SOURCE_CHANGED');return decoder.state;
 }finally{fs.closeSync(fd);}
}
export function openTapeDeltaJournalV1(journal,seed){
 need(journal&&typeof journal.commit==='function'&&typeof journal.hash==='function'&&Number.isSafeInteger(journal.sequence),'JOURNAL_INTERFACE');
 if(journal.sequence===0&&!fs.existsSync(journal.file))fs.writeFileSync(journal.file,'',{flag:'wx',mode:0o600});
 const codec=new TapeCheckpointDeltaV1(seed,journal.sequence),identity=fs.statSync(journal.file);let expectedBytes=identity.size,failed=false;
 let requireFullCheckpoint=journal.sequence===0||(journal.last?.state?.codec!=='tape_market_delta_v1'&&journal.last?.state?.seenIdentityVersion!==2);
 return{get state(){return codec.state;},commit(next,event){
  need(!failed,'FAILED_WRITER');need(journal.sequence===codec.sequence&&event?.type==='anonymous_trade_page'&&event.receivedAt===next.lastAt,'COMMIT_SCOPE');const current=fs.statSync(journal.file);need(current.dev===identity.dev&&current.ino===identity.ino&&current.size===expectedBytes,'WRITER_SOURCE_CHANGED');
  const frame=codec.prepare(next,event.exchangeId),physical=requireFullCheckpoint||frame.sequence%64===0?next:frame;
  try{journal.commit(physical,event);codec.accept(next,frame);expectedBytes=fs.statSync(journal.file).size;requireFullCheckpoint=false;}catch{failed=true;throw Object.assign(Error('TAPE_COMPACT_DURABLE_COMMIT_FAILED'),{durableRecordingFailed:true,fatal:true});}
  return journal.last;
 }};
}
