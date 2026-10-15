import {pathToFileURL} from 'node:url';import {httpsOnce} from './read-get.mjs';
export function authToken(input,options={}){
 if(!input||Object.keys(input).length!==1||!input.headers)throw Error('AUTH_INPUT_INVALID');
 return httpsOnce(new URL('https://sig.thesuper.market/api/v1/realtime/token'),input.headers,{...options,method:'POST',maxBytes:65536});
}
// One original auth request, no HTTP retry. Successful secret content is piped
// privately to the parent collector and must never be logged or persisted.
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 let body='';try{for await(const chunk of process.stdin){body+=chunk;if(body.length>16384)throw Error('INPUT_LIMIT');}const input=JSON.parse(body);body='';process.stdout.write(JSON.stringify(await authToken(input))+'\n');}catch{process.stdout.write(JSON.stringify({status:0,content:null,retryAfterSeconds:60,transport:{category:'transport_input',typed:true,transportCode:'INVALID_INPUT'},dns:[]})+'\n');process.exitCode=1;}
}
