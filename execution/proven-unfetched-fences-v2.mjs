import {provenUnfetchedFencesV1} from './proven-unfetched-fences-v1.mjs';import {payloadHash} from './core.mjs';
export function provenUnfetchedFencesV2(o){
 if(provenUnfetchedFencesV1(o))return true;
 if(!o||o.orderId!==null||o.ack!==null||o.fetchStarts?.length!==0||o.receipts?.length!==0||o.fences?.length!==1)return false;
 return o.physicalFetchBlockProofs?.some(p=>p.kind==='affirmative_before_actual_fetch_clock_block_v1'&&p.physicalFetchReached===false&&p.method==='POST'&&p.endpoint==='/orders'&&p.code==='BAAL_SCALED_PHYSICAL_FETCH_FENCE'&&p.requestBodyHash===o.bodyHash&&p.requestBodyHash===payloadHash(o.body)&&Date.parse(p.at)>=Date.parse(o.fences[0].at))===true;
}
