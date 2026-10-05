import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
const ballots=process.argv.includes('--ballots');
const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime','research-sources-current-polls-20261005'+(ballots?'-ballots':''));
const sources=ballots?[
 ['kansas-official.html','https://meadeco.org/367/2026-General-Election-Information',['2026','Hamilton','Marshall','Graham']]
]:[
 ['kansas-emerson.html','https://emersoncollegepolling.com/kansas-2026/',['Hamilton','Marshall','September 15','750']],
 ['iowa-emerson.html','https://emersoncollegepolling.com/iowa-2026-poll-hinson-leads-turek/',['Hinson','Turek','September 1','750']],
 ['iowa-marist.html','https://maristpoll.marist.edu/polls/2026-midterm-elections-in-iowa-september-2026/',['Turek','Hinson','registered voters','1,050']],
 ['national-emerson.html','https://emersoncollegepolling.com/september-2026-national-poll-democrats/',['53%','42%','September 21','1,000']],
 ['iowa-official.html','https://elections.marshallcountyia.gov/elections/info/2026_general_election_2026_11_03/',['2026','Hinson','Turek','Laehn']]
];
fs.mkdirSync(dir,{recursive:true,mode:0o700});const captures=[];
for(const [file,url,required]of sources){
 const out=path.join(dir,file),receipt=out+'.receipt.json';
 if(fs.existsSync(out)||fs.existsSync(receipt)){
  if(!fs.existsSync(out)||!fs.existsSync(receipt))throw Error('INCOMPLETE_SOURCE_CAPTURE');
  const r=JSON.parse(fs.readFileSync(receipt)),hash=createHash('sha256').update(fs.readFileSync(out)).digest('hex');
  if(hash!==r.contentHash||r.url!==url)throw Error('SOURCE_CAPTURE_CHANGED');captures.push(r);continue;
 }
 const r=await fetch(url,{signal:AbortSignal.timeout(30000)});if(!r.ok)throw Error('PUBLIC_SOURCE_HTTP_'+r.status);
 const bytes=Buffer.from(await r.arrayBuffer());if(bytes.length>4000000)throw Error('PUBLIC_SOURCE_TOO_LARGE');
 const text=bytes.toString('utf8');for(const phrase of required)if(!text.includes(phrase))throw Error('PUBLIC_SOURCE_FACTS_CHANGED_'+file);
 const record={file,url,observedAt:new Date().toISOString(),bytes:bytes.length,contentHash:createHash('sha256').update(bytes).digest('hex'),contentType:r.headers.get('content-type')};
 fs.writeFileSync(out,bytes,{flag:'wx',mode:0o600});fs.writeFileSync(receipt,JSON.stringify(record)+'\n',{flag:'wx',mode:0o600});captures.push(record);
}
const manifest={schemaVersion:1,id:ballots?'current-primary-ballots-20261005-v1':'current-primary-polls-20261005-v1',captures,rawSourcesRestrictedLocally:true,accountApiRequests:0};
const file='research/current-polls-v1/'+(ballots?'ballot-sources.json':'sources.json');if(fs.existsSync(file)){if(JSON.stringify(JSON.parse(fs.readFileSync(file)))!==JSON.stringify(manifest))throw Error('MANIFEST_CHANGED');}else fs.writeFileSync(file,JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({sources:captures.length,accountApiRequests:0,rawSourcesPublished:false}));
