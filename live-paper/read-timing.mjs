import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
export function timingSummary(events){
 const groups={},distribution=a=>{a=a.filter(Number.isFinite).sort((x,y)=>x-y);return {count:a.length,median:a.length?a[Math.floor((a.length-1)*.5)]:null,p95:a.length?a[Math.floor((a.length-1)*.95)]:null,max:a.length?a.at(-1):null};};
 for(const e of events){validateTiming(e);const g=groups[e.kind]??={requests:0,outcomes:{},queueWaitMs:[],admissionWaitMs:[],headersMs:[],bodyMs:[],serviceMs:[],totalMs:[]};g.requests++;g.outcomes[e.outcome]=(g.outcomes[e.outcome]||0)+1;for(const k of ['queueWaitMs','admissionWaitMs','headersMs','bodyMs','serviceMs','totalMs'])g[k].push(e[k]);}
 for(const g of Object.values(groups))for(const k of ['queueWaitMs','admissionWaitMs','headersMs','bodyMs','serviceMs','totalMs'])g[k]=distribution(g[k]);
 return {records:events.length,groups};
}
export function validateTiming(e){
 const fields=['schemaVersion','kind','marketId','queuedAt','admittedAt','finishedAt','queueWaitMs','admissionWaitMs','headersMs','bodyMs','serviceMs','totalMs','statusCode','outcome'];
 if(!e||typeof e!=='object'||Array.isArray(e)||Object.keys(e).some(k=>!fields.includes(k))||e.schemaVersion!==1||!['tournament','positions','orders','book','market','prices','history','fills','pnl'].includes(e.kind)||!['success','connection_error','http_error','body_error','aborted','blocked'].includes(e.outcome))throw Error('Invalid timing schema');
 if(e.marketId!==null&&(typeof e.marketId!=='string'||!/^([1-9]\d*)$/.test(e.marketId)))throw Error('Invalid timing market');
 for(const k of ['queuedAt','finishedAt'])if(typeof e[k]!=='string'||!Number.isFinite(Date.parse(e[k])))throw Error('Invalid timing clock');
 if(e.admittedAt!==null&&(typeof e.admittedAt!=='string'||!Number.isFinite(Date.parse(e.admittedAt))))throw Error('Invalid timing admission');
 for(const k of ['queueWaitMs','admissionWaitMs','headersMs','bodyMs','serviceMs','totalMs'])if(e[k]!==null&&(!Number.isFinite(e[k])||e[k]<0))throw Error('Invalid timing duration');
 if(e.statusCode!==null&&(!Number.isInteger(e.statusCode)||e.statusCode<100||e.statusCode>599))throw Error('Invalid timing status');return e;
}

export function analyzeReadTimings(raw){
 const lastNewline=raw.lastIndexOf('\n'),complete=raw.slice(0,lastNewline+1),events=complete?complete.trimEnd().split('\n').map(line=>validateTiming(JSON.parse(line))):[],outsideClientGaps=[];
 for(let i=1;i<events.length;i++)outsideClientGaps.push(Math.max(0,Date.parse(events[i].queuedAt)-Date.parse(events[i-1].finishedAt)));
 const sorted=outsideClientGaps.sort((a,b)=>a-b);
 return {schemaVersion:1,sourceHash:createHash('sha256').update(complete).digest('hex'),sourceBytes:Buffer.byteLength(complete),partialTailExcluded:complete.length!==raw.length,firstQueuedAt:events[0]?.queuedAt||null,lastFinishedAt:events.at(-1)?.finishedAt||null,...timingSummary(events),outsideClientGapMs:{count:sorted.length,median:sorted.length?sorted[Math.floor((sorted.length-1)*.5)]:null,p95:sorted.length?sorted[Math.floor((sorted.length-1)*.95)]:null,max:sorted.at(-1)??null},interpretation:{networkRequests:0,headersMs:'Time to headers or failed/cancelled header wait',bodyMs:'Time reading and parsing JSON, including failed reads; HTTP-error bodies are not read',queueWaitMs:'Waiting behind earlier calls on this same serialized client',admissionWaitMs:'Pacing/cooldown wait plus admission publication',outsideClientGapMs:'Caller dependency/processing/explicit sleep or restart time; cannot isolate CPU work from backoff',rateOrConcurrencyChanged:false,limitations:['Wall-clock durations, not a server-internal trace','Small correlated samples do not justify a higher request budget','No credentials, URLs, cursors, headers, account bodies or error messages are recorded']}};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){const i=process.argv.indexOf('--file');if(i<0||!process.argv[i+1])throw Error('Required --file');console.log(JSON.stringify(analyzeReadTimings(fs.readFileSync(process.argv[i+1],'utf8')),null,2));}
