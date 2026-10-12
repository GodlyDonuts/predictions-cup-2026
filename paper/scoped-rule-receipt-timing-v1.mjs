import {instant} from './fixed.mjs';
export function scopedRuleReceiptTimingV1({rules,book}) {
 const start=instant(rules.requestStartedAt),known=instant(rules.receivedAt),request=instant(book.requestStartedAt);
 if(start>known||known>request)return{eligible:false,reason:'RULES_NOT_KNOWN_BEFORE_BOOK'};
 if(request-start>300000)return{eligible:false,reason:'RULES_TOO_OLD_AT_BOOK_REQUEST'};
 return{eligible:true,reason:'CAUSAL_RULE_RECEIPT',ageMs:request-start};
}
