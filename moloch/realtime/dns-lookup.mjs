import dns from 'node:dns';import net from 'node:net';

// One resolver fallback for a typed transient lookup failure, before a socket
// opens. Keep the original hostname/SNI and default certificate verification.
// No API request, authentication renewal, persistent cache or external resolver.
export function boundedLookup(record,lookup=dns.lookup,resolve4=dns.resolve4) {
 return (hostname,options,callback)=>lookup(hostname,options,(error,address,family)=>{
  if(error?.code!=='EAI_AGAIN'||options===6||options?.family===6){callback(error,address,family);return;}
  record({dnsFallback:'started'});
  resolve4(hostname,(fallbackError,addresses)=>{
   if(fallbackError){record({dnsFallback:'failed'});callback(fallbackError);return;}
   const valid=Array.isArray(addresses)?addresses.filter(a=>net.isIP(a)===4).slice(0,4):[];
   if(!valid.length){record({dnsFallback:'failed'});const e=Error('DNS_FALLBACK_NO_ADDRESS');e.code='EAI_AGAIN';callback(e);return;}
   record({dnsFallback:'succeeded'});
   if(options?.all)callback(null,valid.map(address=>({address,family:4})));else callback(null,valid[0],4);
  });
 });
}
