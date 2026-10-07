import {payloadHash} from './core.mjs';
// Proof is emitted by the serialized client only when beforeNetwork threw,
// before the physical fetch expression. It cannot describe an API rejection.
export function blockedNetworkProofV1({spec,error,at}){
 if(!['POST','DELETE'].includes(spec?.method)||typeof spec.bodyJson!=='string'
  ||typeof spec.beforeNetwork!=='function'||!Number.isFinite(Date.parse(at)))throw Error('BLOCKED_NETWORK_PROOF_INPUT');
 const raw=error?.code??error?.message;
 return{schemaVersion:1,kind:'serialized_client_before_network_block_v1',method:spec.method,
  endpoint:spec.endpoint,requestBodyHash:payloadHash(JSON.parse(spec.bodyJson)),
  at,physicalFetchReached:false,code:/^[A-Z][A-Z0-9_]{1,120}$/.test(raw??'')?raw:'NETWORK_GUARD_BLOCKED'};
}
export function blockedNetworkProofMatchesV1(proof,intent){
 return proof?.schemaVersion===1&&proof.kind==='serialized_client_before_network_block_v1'
  &&proof.method==='POST'&&proof.endpoint==='/orders'&&proof.physicalFetchReached===false
  &&proof.requestBodyHash===intent?.bodyHash&&proof.requestBodyHash===payloadHash(intent.body)
  &&Date.parse(proof.at)>=Date.parse(intent.createdAt)
  &&intent.orderId===null&&intent.actualFetchCount===0;
}

