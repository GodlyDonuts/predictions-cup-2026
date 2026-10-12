// Isolate a known legacy observer implementation limit. This never treats its
// old accounting as drained, resets a journal, or suppresses integrity faults.
export async function restoreOptionalPaperHostV1(open){
 try{return {host:await open(),unavailable:null};}catch(error){
  if(error.code!=='ERR_STRING_TOO_LONG'||error.durableRecordingFailed||error.fatal)throw error;
  return {host:null,unavailable:{status:'unavailable',reason:'LEGACY_PAPER_STRING_LIMIT',accountingPreserved:true,currentAccountingVerified:false,liabilitiesNotReleased:true,executionAuthority:false}};
 }
}
