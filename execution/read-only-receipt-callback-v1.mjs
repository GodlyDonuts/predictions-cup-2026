export function readOnlyReceiptCallbackV1(host,event){
 if(typeof host?.onFetchStart==='function')return host.onFetchStart(event);
 if(event?.method==='GET')return false;
 throw Object.assign(Error('WRITE_RECEIPT_CALLBACK_REQUIRED'),{fatal:true,durableRecordingFailed:true,mayHaveExecuted:true});
}

