// A body failure never authorizes retrying a write or using stale account data.
export function readBodyFailureV1(cause, {method, aborted=false}) {
 const error=Error('Invalid market API JSON');
 if(cause?.durableRecordingFailed===true||cause?.fatal===true){
  error.durableRecordingFailed=cause.durableRecordingFailed===true;
  error.fatal=true;
  return error;
 }
 const transient=new Set(['UND_ERR_BODY_TIMEOUT','UND_ERR_SOCKET','ECONNRESET','ETIMEDOUT']);
 const code=cause?.code??cause?.cause?.code;
 error.recoverable=method==='GET'&&!aborted&&(cause instanceof SyntaxError||transient.has(code));
 return error;
}

