export function staleFundingFailureCanRetry(error,{journalUnchanged,alreadyFailed}){
 return alreadyFailed!==true&&journalUnchanged===true&&error?.durableRecordingFailed!==true&&error?.mayHaveExecuted!==true
  &&['REST_NO_RECEIPT_TIME','BAAL_SCALED_EVIDENCE_SOURCE_CLOCK'].includes(error?.code??error?.message);
}
