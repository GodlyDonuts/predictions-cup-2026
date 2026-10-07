// Retain stable diagnostic codes, never an arbitrary exception message that
// could contain response data, credentials, paths, or account information.
export function publicationFailure(cause,method){
 const raw=cause?.code??cause?.message;
 const safe=/^[A-Z][A-Z0-9_]{1,120}$/.test(raw??'')?raw:'UNCLASSIFIED_PUBLICATION_FAILURE';
 return Object.assign(Error('Public book recording failed: '+safe),{
  code:safe,publicationOperation:['publishBook','invalidate','close'].includes(method)?method:'unknown',
  durableRecordingFailed:true,fatal:true,
 });
}

