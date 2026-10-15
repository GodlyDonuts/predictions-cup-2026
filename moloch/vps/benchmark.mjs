import http from 'node:http';import fs from 'node:fs';import {admissionAt} from './capture.mjs';
const output=process.argv[2];if(!output)throw Error('benchmark output required');let received=0;const server=http.createServer((req,res)=>{received++;res.end('{}');});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const endpoint=`http://127.0.0.1:${server.address().port}/`,budget={times:[],nextAt:0},cpu=process.cpuUsage(),start=Date.now();
try{for(let i=0;i<100;i++){
 for(;;){const now=Date.now(),at=admissionAt(budget,now);if(at<=now)break;await new Promise(r=>setTimeout(r,at-now));}
 const now=Date.now();budget.times.push(now);await new Promise((resolve,reject)=>{const req=http.get(endpoint,res=>{res.resume();res.once('end',resolve);});req.once('error',reject);});
}}finally{await new Promise(resolve=>server.close(resolve));}
const elapsedMs=Date.now()-start,used=process.cpuUsage(cpu),spacing=budget.times.slice(1).map((t,i)=>t-budget.times[i]);
const report={targetReadsPerMinute:100,loopbackRequests:received,elapsedMs,minimumStartSpacingMs:Math.min(...spacing),cpuMilliseconds:(used.user+used.system)/1000,sigRequests:0,botStarted:false,platform:process.platform,architecture:process.arch};
if(received!==100||report.minimumStartSpacingMs<600)throw Error('PACING_BENCHMARK_FAILED');fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(report));
