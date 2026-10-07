// A failed fresh read packet grants no execution authority. Financial failures
// and any prepared/uncertain intent remain blocking and retain their provenance.
export function genericReadPacketMayRetry(error,status){
 return error?.durableRecordingFailed!==true&&error?.mayHaveExecuted!==true
  &&(error?.code??error?.message)==='REST_NO_RECEIPT_TIME'
  &&status?.phase==='ready'&&!status.intent&&status?.unknown!==true;
}
