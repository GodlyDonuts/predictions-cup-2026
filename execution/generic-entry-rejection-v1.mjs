// Candidate-local read failures can defer that pair without ending discovery.
// Account uncertainty, transport/backoff, durability and any durable intent stop it.
export function genericEntryRejectionMayDefer(error,status){
 return !!error&&!error.fatal&&!error.durableRecordingFailed&&error.retryAfterMs===undefined&&!status?.intent&&status?.unknownAcceptance===false&&status?.phase==='ready'&&/^BAAL_SCALED_EVIDENCE_(ENTRY_[A-Z_]+|CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET)$/.test(error.code??'');
}
