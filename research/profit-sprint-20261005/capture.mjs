import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime','research-profit-sprint-20261005');
fs.mkdirSync(runtime,{recursive:true,mode:0o700});
const sources=[
 ['fox-texas-topline.pdf','https://static.foxnews.com/foxnews.com/content/uploads/2026/10/fox_september-24-28-2026_texas_topline_october-1-release.pdf'],
 ['fox-texas-crosstabs.pdf','https://static.foxnews.com/foxnews.com/content/uploads/2026/10/fox_september-24-28-2026_texas_cross-tabs_october-1-release.pdf'],
 ['fox-iowa-topline.pdf','https://static.foxnews.com/foxnews.com/content/uploads/2026/10/fox_september-24-28-2026_iowa_topline_october-1-release.pdf'],
 ['fox-michigan-topline.pdf','https://static.foxnews.com/foxnews.com/content/uploads/2026/09/fox_september-24-28-2026_michigan_topline_september-30-release.pdf'],
 ['bigdata-texas.html','https://www.bigdatapoll.com/blog/lone-star-state-poll-talarico-paxton-in-close-race-us-senate-in-texas/'],
 ['ut-governor-tracker.html','https://texaspolitics.utexas.edu/blog/texas-2026-gubernatorial-poll-tracker'],
 ['tsu-september.pdf','https://static1.squarespace.com/static/67aa7aa3e284b002cd6a420e/t/6ab15cb39e4cc420b9ef07f5/1790008500236/TSUSept2026_TXStatewideRaces.pdf'],
 ['siena-governors.html','https://sri.siena.edu/2026/10/04/nytimes-siena-poll-of-likely-voters-gubernatorial-races-2/'],
 ['siena-senate.html','https://sri.siena.edu/2026/10/03/nytimes-siena-poll-of-likely-voters-senate-races/'],
 ['nyt-texas-toplines.html','https://www.nytimes.com/interactive/2026/10/03/polls/times-siena-texas-poll-toplines.html'],
 ['tpor-september.html','https://texaspublicopinionresearch.substack.com/p/new-poll-in-texas-general-election-662'],
];
const captures=await Promise.all(sources.map(async([file,url])=>{
 const out=path.join(runtime,file),receipt=out+'.receipt.json';
 if(fs.existsSync(receipt)){const r=JSON.parse(fs.readFileSync(receipt));if(r.status==='captured'&&createHash('sha256').update(fs.readFileSync(out)).digest('hex')!==r.contentHash)throw Error('FROZEN_SOURCE_CHANGED');return r;}
 let record;try{const r=await fetch(url,{signal:AbortSignal.timeout(20000)});const bytes=Buffer.from(await r.arrayBuffer());if(!r.ok)throw Error('SOURCE_HTTP_'+r.status);if(bytes.length<100||bytes.length>6000000)throw Error('SOURCE_SIZE');if(file.endsWith('.pdf')&&bytes.subarray(0,5).toString()!=='%PDF-')throw Error('NOT_PDF');fs.writeFileSync(out,bytes,{flag:'wx',mode:0o600});record={file,url,status:'captured',observedAt:new Date().toISOString(),bytes:bytes.length,contentHash:createHash('sha256').update(bytes).digest('hex'),contentType:r.headers.get('content-type')};}catch(e){record={file,url,status:'unavailable',observedAt:new Date().toISOString(),reason:/SOURCE_HTTP_\d+|SOURCE_SIZE|NOT_PDF/.exec(e.message)?.[0]||'PUBLIC_FETCH_FAILED'};}
 fs.writeFileSync(receipt,JSON.stringify(record,null,2)+'\n',{flag:'wx',mode:0o600});return record;
}));
const manifest={schemaVersion:1,id:'profit-sprint-primary-sources-20261005',at:new Date().toISOString(),captures,accountApiRequests:0,rawSourcesRestrictedLocally:true,executionAuthority:false};
fs.writeFileSync('research/profit-sprint-20261005/sources.json',JSON.stringify(manifest,null,2)+'\n');
console.log(JSON.stringify(captures.map(x=>({file:x.file,status:x.status,reason:x.reason}))));
