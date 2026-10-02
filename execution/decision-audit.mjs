import fs from 'node:fs';import {Journal} from '../live-paper/runtime.mjs';
const codes=new Set(['REFERENCE_BLOCKED','WINDOW_BLOCKED','UNKNOWN_IDENTITY','STATE_CONCENTRATION','EXTERNAL_EXPOSURE','COOLDOWN','SIGNAL_BLOCKED','ZERO_EXECUTABLE_SIZE','PREPARE_REJECTED','INTENT_READY']);
const instant=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
const number=x=>typeof x==='number'&&Number.isFinite(x)?x:null;
export function assessEntryGates(reference,ask,bid,policy){
 if(!reference.ready)return {referenceReady:false,referenceReason:typeof reference.reason==='string'&&/^[a-z_]{1,100}$/.test(reference.reason)?reference.reason:'unavailable',failed:['reference'],reference:null};
 const discount=reference.center-ask,relative=discount/Math.max(reference.center,.001),z=discount/reference.robustDeviation;
 const tests={price:ask>=policy.minimumPrice&&ask<=policy.maximumPrice,spread:ask>bid&&ask-bid<=policy.maximumSpread,minimumDiscount:discount>=policy.minimumDip,relativeDiscount:relative>=policy.minimumRelativeDip,maximumDiscount:discount<=policy.maximumDip,robustZ:z>=policy.minimumZ};
 return {referenceReady:true,referenceReason:null,reference:{center:number(reference.center),robustDeviation:number(reference.robustDeviation),samples:Number.isSafeInteger(reference.samples)?reference.samples:null,firstAt:instant(reference.firstAt)?reference.firstAt:null,lastAt:instant(reference.lastAt)?reference.lastAt:null,discount:number(discount),relativeDiscount:number(relative),z:number(z),effectiveAsk:number(ask),effectiveBid:number(bid)},failed:Object.keys(tests).filter(k=>!tests[k]),tests};
}
function curatedAssessment(value){
 if(value===null)return null;const allowed=['referenceReady','referenceReason','reference','failed','tests'],gates=['price','spread','minimumDiscount','relativeDiscount','maximumDiscount','robustZ'];
 if(!value||typeof value!=='object'||Object.keys(value).some(k=>!allowed.includes(k))||typeof value.referenceReady!=='boolean'||!Array.isArray(value.failed)||value.failed.some(k=>!['reference',...gates].includes(k)))throw Error('INVALID_ENTRY_ASSESSMENT');
 if(value.referenceReason!==null&&(typeof value.referenceReason!=='string'||!/^[a-z_]{1,100}$/.test(value.referenceReason)))throw Error('INVALID_ENTRY_ASSESSMENT');
 if(value.reference!==null){const fields=['center','robustDeviation','samples','firstAt','lastAt','discount','relativeDiscount','z','effectiveAsk','effectiveBid'];if(!value.reference||Object.keys(value.reference).length!==fields.length||Object.keys(value.reference).some(k=>!fields.includes(k)))throw Error('INVALID_ENTRY_ASSESSMENT');for(const [k,v]of Object.entries(value.reference))if(v!==null&&(['firstAt','lastAt'].includes(k)?!instant(v):typeof v!=='number'||!Number.isFinite(v)))throw Error('INVALID_ENTRY_ASSESSMENT');}
 if(value.tests!==undefined&&(!value.tests||Object.keys(value.tests).length!==gates.length||Object.entries(value.tests).some(([k,v])=>!gates.includes(k)||typeof v!=='boolean')))throw Error('INVALID_ENTRY_ASSESSMENT');return structuredClone(value);
}
export function normalizedEntryDecision({runId,book,clockAt,code,modelHash=null,referenceProof=null,assessment=null,detailCode=null}){
 if(typeof runId!=='string'||!/^sig-pilot-[a-z0-9-]+$/.test(runId)||!codes.has(code)||!instant(clockAt)||!instant(book?.receivedAt)||!instant(book?.requestStartedAt)||Date.parse(book.requestStartedAt)>Date.parse(book.receivedAt)||Date.parse(book.receivedAt)>Date.parse(clockAt)||!/^([1-9]\d*)$/.test(book.marketId)||!/^([1-9]\d*)$/.test(book.exchangeId)||typeof book.versionKey!=='string'||book.versionKey.length>256||modelHash!==null&&!/^[a-f0-9]{64}$/.test(modelHash))throw Error('INVALID_ENTRY_DECISION');
 const prices={bid:book.bids?.[0]?.price,ask:book.asks?.[0]?.price};for(const p of Object.values(prices))if(typeof p!=='string'||!/^(?:0\.\d{6}|1\.000000)$/.test(p))throw Error('INVALID_ENTRY_DECISION_PRICE');
 const quantities={bestBidQuantity:book.bids[0].quantity,bestAskQuantity:book.asks[0].quantity};if(Object.values(quantities).some(q=>!Number.isSafeInteger(q)||q<1||q>2147483647))throw Error('INVALID_ENTRY_DECISION_DEPTH');
 const proof=referenceProof===null?null:{bankHash:referenceProof.bankHash,sourceSequence:referenceProof.sourceSequence,sourceHash:referenceProof.sourceHash,referenceHash:referenceProof.referenceHash};if(proof&&(!Number.isSafeInteger(proof.sourceSequence)||proof.sourceSequence<1||Object.entries(proof).some(([k,v])=>k!=='sourceSequence'&&!/^[a-f0-9]{64}$/.test(v))))throw Error('INVALID_ENTRY_DECISION_PROOF');
 if(detailCode!==null&&(typeof detailCode!=='string'||!/^[A-Z][A-Z0-9_]{0,100}$/.test(detailCode)))throw Error('INVALID_ENTRY_DECISION_DETAIL');
 return {schemaVersion:1,runId,clockAt,code,marketId:book.marketId,exchangeId:book.exchangeId,requestStartedAt:book.requestStartedAt,receivedAt:book.receivedAt,versionKey:book.versionKey,...prices,...quantities,modelHash,referenceProof:proof,assessment:curatedAssessment(assessment),detailCode,eligibleForExecution:false};
}
export function openEntryDecisionAudit(file,runId){
 const journal=new Journal(file);let counts={},last=null;
 if(journal.last&&journal.last.event?.decision?.runId!==runId)throw Error('ENTRY_DECISION_RUN_CHANGED');
 if(fs.existsSync(file))for(const line of fs.readFileSync(file,'utf8').split('\n').filter(Boolean)){const row=JSON.parse(line),d=row.event?.decision;if(row.event?.type!=='entry_decision'||d?.runId!==runId||!codes.has(d.code))throw Error('ENTRY_DECISION_JOURNAL_SCHEMA');counts[d.code]=(counts[d.code]||0)+1;last=d;}
 return {record(input){const decision=normalizedEntryDecision({...input,runId});journal.commit(null,{type:'entry_decision',decision});const fd=fs.openSync(file,'r+');try{fs.fsyncSync(fd);}finally{fs.closeSync(fd);}counts[decision.code]=(counts[decision.code]||0)+1;last=decision;return decision;},status:()=>({sequence:journal.sequence,hash:journal.previous,counts,lastDecision:last,networkRequests:0,executionAuthority:false})};
}
