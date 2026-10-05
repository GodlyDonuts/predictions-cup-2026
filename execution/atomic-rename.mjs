import fs from 'node:fs';
const retryCodes=new Set(['EPERM','EACCES','EBUSY']);
const waitBuffer=new Int32Array(new SharedArrayBuffer(4));
// Windows may briefly deny replacement while a reader or scanner holds the
// destination. Retry the same already-synced temporary file. Never unlink the
// existing projection or repeat the authoritative journal operation.
export function replaceFileSync(source,target,{platform=process.platform,renameSync=fs.renameSync,sleep=ms=>Atomics.wait(waitBuffer,0,0,ms)}={}){
 const delays=[10,20,40,80,160];
 for(let attempt=0;;attempt++){
  try{renameSync(source,target);return{attempts:attempt+1};}
  catch(error){if(platform!=='win32'||!retryCodes.has(error.code)||attempt>=delays.length)throw error;sleep(delays[attempt]);}
 }
}
