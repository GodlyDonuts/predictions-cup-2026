import {payloadHash} from './core.mjs';
export function provenUnfetchedFencesV1(order){
 if(!Array.isArray(order?.fences)||order.fences.length>1||order.fetchStarts?.length!==0||order.receipts?.length!==0)return false;
 if(order.fences.length===0)return true;
 const p=order.blockedNetworkProofs?.at(-1),f=order.fences[0];
 return p?.kind==='serialized_client_before_network_block_v1'&&p.method==='POST'&&p.endpoint==='/orders'&&p.physicalFetchReached===false&&p.requestBodyHash===order.bodyHash&&p.requestBodyHash===payloadHash(order.body)&&Date.parse(p.at)>=Date.parse(f.at);
}

