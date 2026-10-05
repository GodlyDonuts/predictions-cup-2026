import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime/research-sources-current-marist-20261005-v2');
const sources=[
 ['michigan-emerson.html','https://emersoncollegepolling.com/michigan-2026-poll-el-sayed-and-rogers-locked-in-close-election/',['El-Sayed','Rogers','48%','46%','September 12','1,000']],
 ['texas-emerson.html','https://emersoncollegepolling.com/texas-2026-poll-talarico-and-paxton-in-dead-heat-abbott-maintains-edge/',['Talarico','Paxton','47%','46%','September 12','1,000']],
 ['texas-official.pdf','https://www.sos.state.tx.us/elections/forms/2026-ballot-cert.pdf',null],
 ['michigan-official.html','https://mi-boe.entellitrak.com/etk-mi-boe-prod/page.request.do?electionType=GEN&electionYear=2026&page=page.miboePublicReport',['El-Sayed','Rogers','General Election']]
];
const captures=[];
for(const[file,url,phrases]of sources){const out=path.join(dir,file),receipt=out+'.receipt.json';
 if(fs.existsSync(out)){const r=JSON.parse(fs.readFileSync(receipt));if(r.url!==url||r.contentHash!==createHash('sha256').update(fs.readFileSync(out)).digest('hex'))throw Error('SOURCE_CHANGED');captures.push(r);continue;}
 const r=await fetch(url,{signal:AbortSignal.timeout(30000)});if(!r.ok)throw Error('SOURCE_HTTP_'+r.status);const bytes=Buffer.from(await r.arrayBuffer());const maximum=file==='texas-official.pdf'?32000000:4000000;if(bytes.length>maximum||!bytes.length)throw Error('SOURCE_SIZE_'+file);
 if(phrases){for(const phrase of phrases)if(!bytes.toString().includes(phrase))throw Error('SOURCE_FACT_CHANGED_'+file);}else if(bytes.subarray(0,5).toString()!=='%PDF-')throw Error('SOURCE_NOT_PDF');
 const record={file,url,observedAt:new Date().toISOString(),bytes:bytes.length,contentType:r.headers.get('content-type'),contentHash:createHash('sha256').update(bytes).digest('hex')};fs.writeFileSync(out,bytes,{flag:'wx',mode:0o600});fs.writeFileSync(receipt,JSON.stringify(record)+'\n',{flag:'wx',mode:0o600});captures.push(record);
}
const manifest={schemaVersion:1,id:'current-primary-comparators-20261005-v2',captures,accountApiRequests:0,rawSourcesRestrictedLocally:true},out='research/current-polls-v2/comparator-sources.json';if(fs.existsSync(out)){if(JSON.stringify(JSON.parse(fs.readFileSync(out)))!==JSON.stringify(manifest))throw Error('MANIFEST_CHANGED');}else fs.writeFileSync(out,JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({sources:captures.length,accountApiRequests:0}));
