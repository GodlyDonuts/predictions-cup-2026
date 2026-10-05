import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime/research-sources-current-marist-20261005-v2');
const sources=[
 ['michigan-marist.html','https://maristpoll.marist.edu/polls/midterm-elections-in-michigan-september-2026/',['El-Sayed','Rogers','1,235','September 27']],
 ['michigan-marist.pdf','https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll_MI-NOS-and-Tables_202609281401@9159_7e99y.pdf',null],
 ['texas-marist.html','https://maristpoll.marist.edu/polls/2026-midterm-elections-in-texas-september-2026/',['Talarico','Paxton','1,139','September 20']],
 ['texas-marist.pdf','https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll_TX-NOS-and-Tables_202609221131.pdf',null],
 ['ohio-marist.html','https://maristpoll.marist.edu/polls/midterm-elections-in-ohio-september-2026/',['Brown','Husted','1,298','September 27']],
 ['ohio-marist.pdf','https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll_OH-NOS-and-Tables_202609281347aC909p6465.pdf',null]
];
fs.mkdirSync(dir,{recursive:true,mode:0o700});const captures=[];
for(const[file,url,phrases]of sources){const out=path.join(dir,file),receipt=out+'.receipt.json';
 if(fs.existsSync(out)){const record=JSON.parse(fs.readFileSync(receipt));if(record.url!==url||record.contentHash!==createHash('sha256').update(fs.readFileSync(out)).digest('hex'))throw Error('FROZEN_SOURCE_CHANGED');captures.push(record);continue;}
 const r=await fetch(url,{signal:AbortSignal.timeout(30000)});if(!r.ok)throw Error('SOURCE_HTTP_'+r.status);
 const bytes=Buffer.from(await r.arrayBuffer());if(bytes.length>4000000||!bytes.length)throw Error('SOURCE_SIZE');
 if(phrases){for(const phrase of phrases)if(!bytes.toString().includes(phrase))throw Error('SOURCE_FACT_CHANGED_'+file);}else if(bytes.subarray(0,5).toString()!=='%PDF-')throw Error('SOURCE_NOT_PDF');
 const record={file,url,observedAt:new Date().toISOString(),bytes:bytes.length,contentType:r.headers.get('content-type'),contentHash:createHash('sha256').update(bytes).digest('hex')};
 fs.writeFileSync(out,bytes,{flag:'wx',mode:0o600});fs.writeFileSync(receipt,JSON.stringify(record)+'\n',{flag:'wx',mode:0o600});captures.push(record);
}
const manifest={schemaVersion:1,id:'current-primary-marist-20261005-v2',captures,rawSourcesRestrictedLocally:true,accountApiRequests:0};const out='research/current-polls-v2/sources.json';
if(fs.existsSync(out)){if(JSON.stringify(JSON.parse(fs.readFileSync(out)))!==JSON.stringify(manifest))throw Error('FROZEN_MANIFEST_CHANGED');}else fs.writeFileSync(out,JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({sources:captures.length,accountApiRequests:0,rawPublished:false}));
