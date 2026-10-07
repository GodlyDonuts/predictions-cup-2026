import fs from 'node:fs';import path from 'node:path';import {blockedNetworkProofV1} from './blocked-network-proof-v1.mjs';
// Install when creating a spec, before registering its callback identity.
export function provenNetworkGuardV1({spec,getGuard,proofFile,now=Date.now}){
 if(!path.isAbsolute(proofFile)||typeof getGuard!=='function')throw Error('PROVEN_GUARD_INPUT');
 return async()=>{
  try{return await getGuard()();}catch(error){
   const proof=blockedNetworkProofV1({spec,error,at:new Date(now()).toISOString()}),fd=fs.openSync(proofFile,'a',0o600);
   try{fs.writeSync(fd,JSON.stringify(proof)+'\n');fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
   throw error;
  }
 };
}

