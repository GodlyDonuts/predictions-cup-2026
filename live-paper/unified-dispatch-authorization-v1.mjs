// Exact permits from the new controller already check the shared owner and its
// financial window. Avoid replaying retired actors before routing that permit.
import {isRealtimeTokenDispatch} from './realtime-client.mjs';
export function unifiedDispatchAuthorizationV1({spec,readOnly,human,atomic,paired,sharedExitAllowed,legacyAuthorize,captureTokenAuthorize}){
 if(readOnly&&spec.method!=='GET')return isRealtimeTokenDispatch(spec)&&captureTokenAuthorize?.(spec)===true;
 if(atomic?.authorizes(spec)===true)return true;
 if(human?.blocksLegacyWrites()&&spec.method!=='GET'&&human.authorizes(spec)!==true&&sharedExitAllowed(spec)!==true)return false;
 return human?.authorizes(spec)===true||paired?.authorizes(spec)===true||((spec.method==='GET'||paired?.allowsLegacy(spec)!==false)&&legacyAuthorize(spec)===true);
}
