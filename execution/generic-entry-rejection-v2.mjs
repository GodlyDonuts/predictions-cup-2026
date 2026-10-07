// A foreign-held candidate must be skipped, never incorporated into custody.
// All account uncertainty, persistence, backoff and active-intent gates remain.
export function genericEntryRejectionMayDeferV2(error,status){
 return !!error&&!error.fatal&&!error.durableRecordingFailed&&error.retryAfterMs===undefined&&!status?.intent&&status?.unknownAcceptance===false&&status?.phase==='ready'&&/^BAAL_SCALED_EVIDENCE_(ENTRY_[A-Z_]+|CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET|BUY_TARGET_NOT_FLAT)$/.test(error.code??'');
}
