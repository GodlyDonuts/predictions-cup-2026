import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime','poll-calibration-sources-20261005');
const sha=b=>createHash('sha256').update(b).digest('hex');
fs.mkdirSync(dir,{recursive:true,mode:0o700});
const inputs=[
 {file:'raw_polls.csv',url:'https://raw.githubusercontent.com/fivethirtyeight/data/master/pollster-ratings/raw_polls.csv',gitBlob:'08735ea503ad64c67cfa285fd75c0a270fdca245',kind:'csv'},
 {file:'codebook.md',url:'https://raw.githubusercontent.com/fivethirtyeight/data/master/pollster-ratings/README.md',gitBlob:'78b95c4a883383322bf4c4b3f88d69ee4c1e2e48',kind:'text'},
 {file:'marist-iowa-tables.pdf',url:'https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll-IA-NOS-and-Tables_202609211631-1.pdf',kind:'pdf'}
];
const captures=[];
for(const input of inputs){
 const file=path.join(dir,input.file),receipt=file+'.receipt.json';
 if(fs.existsSync(file)||fs.existsSync(receipt)){
  if(!fs.existsSync(file)||!fs.existsSync(receipt))throw Error('INCOMPLETE_CAPTURE');
  const old=JSON.parse(fs.readFileSync(receipt));if(old.url!==input.url||sha(fs.readFileSync(file))!==old.sha256||input.gitBlob&&old.gitBlob!==input.gitBlob)throw Error('CAPTURE_CHANGED');captures.push(old);continue;
 }
 const response=await fetch(input.url,{signal:AbortSignal.timeout(30000)});if(!response.ok)throw Error('SOURCE_HTTP_'+response.status);
 const chunks=[];let length=0;for await(const chunk of response.body){length+=chunk.length;if(length>8000000)throw Error('SOURCE_SIZE_LIMIT');chunks.push(chunk);}
 const bytes=Buffer.concat(chunks),gitBlob=createHash('sha1').update(Buffer.from('blob '+bytes.length+'\0')).update(bytes).digest('hex');
 if(input.gitBlob&&gitBlob!==input.gitBlob)throw Error('GITHUB_BLOB_CHANGED');
 if(input.kind==='csv'&&!bytes.toString('utf8',0,400).includes('margin_actual'))throw Error('CSV_HEADER_CHANGED');
 if(input.kind==='pdf'&&bytes.toString('ascii',0,5)!=='%PDF-')throw Error('PDF_TYPE_CHANGED');
 const record={file:input.file,url:input.url,observedAt:new Date().toISOString(),bytes:bytes.length,sha256:sha(bytes),gitBlob:input.gitBlob??null,contentType:response.headers.get('content-type')};
 fs.writeFileSync(file,bytes,{flag:'wx',mode:0o600});fs.writeFileSync(receipt,JSON.stringify(record,null,2)+'\n',{flag:'wx',mode:0o600});captures.push(record);
}
const manifest={schemaVersion:1,id:'primary-poll-calibration-sources-20261005-v1',captures,rawSourcesRestrictedLocally:true,accountApiRequests:0};
const file='research/poll-calibration-v1/sources.json';if(fs.existsSync(file)){if(JSON.stringify(JSON.parse(fs.readFileSync(file)))!==JSON.stringify(manifest))throw Error('MANIFEST_CHANGED');}else fs.writeFileSync(file,JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({sources:captures.map(c=>({file:c.file,bytes:c.bytes,sha256:c.sha256})),accountApiRequests:0}));
