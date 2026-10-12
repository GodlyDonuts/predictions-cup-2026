import {instant} from './fixed.mjs';
export function makerPlacementLifetimeV1(quotes,{at,latencyMs,minimumActiveMs}){
 const now=instant(at);if(!Array.isArray(quotes)||!Number.isSafeInteger(latencyMs)||latencyMs<1||!Number.isSafeInteger(minimumActiveMs)||minimumActiveMs<1)throw Error('MAKER_PLACEMENT_LIFETIME_POLICY');
 return quotes.filter(q=>instant(q.validUntil)-now>=latencyMs+minimumActiveMs);
}
