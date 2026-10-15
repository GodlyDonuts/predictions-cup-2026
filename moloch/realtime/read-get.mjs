import https from 'node:https';
import {pathToFileURL} from 'node:url';
import {boundedLookup} from './dns-lookup.mjs';

export function validateRead(input) {
 if(!input || typeof input.endpoint!=='string' || input.method && input.method!=='GET') throw Error('READ_INPUT_INVALID');
 const endpoint=input.endpoint;
 if(!/^\/(markets\/[1-9][0-9]*\/orderbook|exchanges\/[1-9][0-9]*\/trades)\?[^#\\\r\n]*$/.test(endpoint)) throw Error('READ_ROUTE_INVALID');
 const url=new URL('https://sig.thesuper.market/api/v1'+endpoint);
 const book=url.pathname.includes('/orderbook');
 const allowed=new Set(book?['tournamentId','depth']:['tournamentId','from','to','limit','cursor']);
 const seen=new Set();
 for(const [key,value] of url.searchParams){if(!allowed.has(key)||seen.has(key)||!value||/[\r\n]/.test(value))throw Error('READ_QUERY_INVALID');seen.add(key);}
 if(!/^[a-f0-9-]{36}$/.test(url.searchParams.get('tournamentId')??''))throw Error('READ_TOURNAMENT_INVALID');
 if(book&&!['20','200'].includes(url.searchParams.get('depth')))throw Error('READ_DEPTH_INVALID');
 if(!book){if(url.searchParams.get('limit')!=='200')throw Error('READ_LIMIT_INVALID');for(const key of ['from','to'])if(!Number.isFinite(Date.parse(url.searchParams.get(key))))throw Error('READ_WINDOW_INVALID');if(Date.parse(url.searchParams.get('from'))>=Date.parse(url.searchParams.get('to')))throw Error('READ_WINDOW_INVALID');}
 if(!input.headers||Object.keys(input.headers).length!==1||typeof input.headers.Authorization!=='string'||!/^Bearer [^\s\r\n]+$/.test(input.headers.Authorization))throw Error('READ_AUTH_INVALID');
 return url;
}

const codes=new Set(['EAI_AGAIN','ENOTFOUND','ECONNRESET','ECONNREFUSED','ETIMEDOUT','EPIPE','ERR_TLS_CERT_ALTNAME_INVALID','CERT_HAS_EXPIRED','UNABLE_TO_VERIFY_LEAF_SIGNATURE','READ_TIMEOUT','READ_BODY_LIMIT','READ_BODY_ABORTED']);
export function readGet(input,{request=https.request,lookup,timeoutMs=30000,maxBytes=16*1024*1024}={}) {
 const url=validateRead(input),dns=[];
 return new Promise(resolve=>{
  let req,timer,done=false;
  const finish=packet=>{if(done)return;done=true;clearTimeout(timer);resolve({...packet,dns});};
  const failure=error=>finish({status:0,content:null,retryAfterSeconds:60,transport:{category:['EAI_AGAIN','ENOTFOUND'].includes(error?.code)?'transport_dns':'transport_network',typed:true,transportCode:codes.has(error?.code)?error.code:'OTHER'}});
  timer=setTimeout(()=>{failure({code:'READ_TIMEOUT'});req?.destroy();},timeoutMs);
  try{
   req=request(url,{method:'GET',headers:{Authorization:input.headers.Authorization},lookup:lookup??boundedLookup(e=>dns.push(e)),rejectUnauthorized:true},res=>{
    let size=0;const chunks=[];
    res.on('data',chunk=>{if(done)return;size+=chunk.length;if(size>maxBytes){failure({code:'READ_BODY_LIMIT'});res.destroy();req.destroy();return;}chunks.push(chunk);});
    res.on('error',failure);res.on('aborted',()=>failure({code:'READ_BODY_ABORTED'}));
    res.on('end',()=>{
     if(!res.complete){failure({code:'READ_BODY_ABORTED'});return;}
     const raw=res.headers['retry-after'];const seconds=/^\d+$/.test(String(raw))?Number(raw):(Date.parse(raw)-Date.now())/1000;
     const delay=Number.isFinite(seconds)?Math.max(60,Math.ceil(seconds)):60;
     // Error bodies can reflect credentials. Retain content only on success.
     finish({status:res.statusCode,content:res.statusCode===200?Buffer.concat(chunks).toString('utf8'):null,retryAfterSeconds:delay,transport:null});
    });
   });
   req.on('error',failure);req.end();
  }catch(error){failure(error);}
 });
}

if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 let body='';try{for await(const chunk of process.stdin){body+=chunk;if(body.length>16384)throw Error('INPUT_LIMIT');}const input=JSON.parse(body);body='';process.stdout.write(JSON.stringify(await readGet(input))+'\n');}catch{process.stdout.write(JSON.stringify({status:0,content:null,retryAfterSeconds:60,transport:{category:'transport_input',typed:true,transportCode:'INVALID_INPUT'},dns:[]})+'\n');process.exitCode=1;}
}
