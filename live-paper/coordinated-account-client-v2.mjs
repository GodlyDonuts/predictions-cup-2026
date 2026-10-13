// A faster completion gap inside the SAME serialized account client. Durable
// rolling admission caps retain headroom below published100READ/30WRITE limits.
import fs from 'node:fs';import path from 'node:path';
import {ReadClient} from './transport-runtime.mjs';import {atomicJson} from '../execution/ledger.mjs';
const need=(v,c)=>{if(!v)throw Error('COORDINATED_CLIENT_'+c);};
export class CoordinatedAccountClientV2 extends ReadClient {
 constructor(key,{admissionFile,coordinatedReadGapMs=100,coordination,...options}){
  need(path.isAbsolute(admissionFile)&&coordinatedReadGapMs===100&&options.networkBudget?.schemaVersion===4&&options.networkBudget.soleAccountClient===true&&coordination?.kind==='baal_single_client_rolling_policy_v1'&&coordination.readCompletionGapMs===100&&coordination.writeCompletionGapMs===2500&&coordination.maxReadsPerMinute===60&&coordination.maxWritesPerMinute===24,'EXPLICIT_COORDINATION');
  const suppliedFetch=options.fetchFn??fetch;super(key,options);this.fetch=async(url,request)=>{await this.admit((request?.method??'GET').toUpperCase());return suppliedFetch(url,request);};this.admissionFile=admissionFile;
  // Keep the nested fast gap as a lower bound; it must not override the
  // shared interval loaded and validated by ReadClient.
  this.interval=Math.max(this.interval,coordinatedReadGapMs);this.readAdmissions=[];this.writeAdmissions=[];
  if(fs.existsSync(admissionFile)){const r=JSON.parse(fs.readFileSync(admissionFile));need(r.schemaVersion===1&&Array.isArray(r.reads)&&Array.isArray(r.writes),'PERSISTENT_STATE');for(const[list,maximum]of[[r.reads,60],[r.writes,24]])need(list.length<=maximum&&list.every((x,i)=>Number.isSafeInteger(x)&&x>=0&&x<=this.now()&&(!i||x>=list[i-1])),'ADMISSION_CLOCK_OR_CAP');this.readAdmissions=r.reads;this.writeAdmissions=r.writes;}
 }
 async admit(method){const list=method==='GET'?this.readAdmissions:this.writeAdmissions,maximum=method==='GET'?60:24;while(true){const at=this.now();while(list.length&&at-list[0]>=60000)list.shift();if(list.length<maximum)break;await this.pause(list[0]+60000-at);}const at=this.now();list.push(at);this.readAdmissions=this.readAdmissions.filter(x=>at-x<60000);this.writeAdmissions=this.writeAdmissions.filter(x=>at-x<60000);atomicJson(this.admissionFile,{schemaVersion:1,reads:this.readAdmissions,writes:this.writeAdmissions});}
}
