import {instant} from './fixed.mjs';
export function makerAdmissionLifetimeV1(quotes,{at,latencyMs,minimumActiveMs}){
 const now=instant(at);if(!Array.isArray(quotes)||!Number.isSafeInteger(latencyMs)||latencyMs<1||!Number.isSafeInteger(minimumActiveMs)||minimumActiveMs<1)throw Error('MAKER_ADMISSION_POLICY');
 return quotes.filter(q=>instant(q.admissionValidUntil)===instant(q.validUntil)&&instant(q.validUntil)>now&&instant(q.orderExpiresAt)-now>=latencyMs+minimumActiveMs);
}
