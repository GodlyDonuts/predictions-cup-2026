import {digest,normalizeBook} from './model.mjs';import {invalidateBasket} from './basket-observer.mjs';import {observeCompleteBasketPair} from './basket-pair-observer.mjs';
const clock=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
// Separate successor projection; the original observer and journal stay intact.
export function advanceCompleteBasketRecord(input,record,bank){
 if(input.bankHash!==digest(bank)||record.sequence!==input.sourceSequence+1||record.previous!==input.lastRecordHash)throw Error('BASKET_PAIR_SOURCE_CONTINUITY');
 const state=structuredClone(input),observations=[],e=record.event,at=e.clockAt||record.at;if(!clock(at))throw Error('BASKET_PAIR_SOURCE_CLOCK');
 const invalidate=p=>{for(const leg of p.bank.legs)state.pairs[p.id]=invalidateBasket(state.pairs[p.id],leg.marketId,at,p.bank);};
 if(Date.parse(at)>Date.parse(bank.cutoff)){
  if(e.type==='account_read_outage')for(const p of bank.pairs)invalidate(p);
  if(e.type==='cycle'){
   if(!Array.isArray(e.accountExclusions)||e.accountExclusions.some(id=>typeof id!=='string')||!clock(e.exclusionObservedAt)||Date.parse(e.exclusionObservedAt)>Date.parse(at)||Object.values(state.pairs).some(s=>s.lastAt&&Date.parse(s.lastAt)>Date.parse(e.exclusionObservedAt)))for(const p of bank.pairs)invalidate(p);
   else{const excluded=new Set(e.accountExclusions),snapshots=[...(e.sourceSnapshots||[]),...(e.supplementalDepth||[])],measurement=e.basketMeasurement;
    for(const p of bank.pairs){if(p.bank.legs.some(l=>excluded.has(l.exchangeId))){invalidate(p);continue;}
     const matches=snapshots.filter(r=>p.bank.legs.some(l=>l.marketId===(r.marketId||/^\/markets\/([1-9]\d*)\/orderbook\?/.exec(r.endpoint||'')?.[1])));
     if(!matches.length)continue;
     const ids=measurement?.marketIds;
     // Arbitrary single-leg/foreign-cycle quotes cannot seed a paired claim.
     if(!['paired_depth_measurement','paired_depth_followup'].includes(measurement?.reason)||!Array.isArray(ids)||ids.length!==2||new Set(ids).size!==2||p.bank.legs.some(l=>!ids.includes(l.marketId))||matches.length!==2){invalidate(p);continue;}
     try{const books=matches.map(r=>{const id=/^\/markets\/([1-9]\d*)\/orderbook\?/.exec(r.endpoint||'')?.[1];if(r.skipped||!clock(r.receivedAt)||Date.parse(r.receivedAt)>Date.parse(at)||Date.parse(r.receivedAt)<Date.parse(e.exclusionObservedAt)||r.endpoint!=='/markets/'+id+'/orderbook?tournamentId='+p.bank.tournamentId+'&depth=20')throw Error('INVALID_PAIRED_SOURCE');return {...normalizeBook(r.data,id,p.bank.tournamentId,r.receivedAt,r.requestStartedAt,20000,excluded,'authoritative_receipt_v1'),tournamentId:p.bank.tournamentId};});
      const result=observeCompleteBasketPair(state.pairs[p.id],books,p.bank);state.pairs[p.id]=result.state;observations.push({pairId:p.id,...result.observation,sourceSequence:record.sequence,sourceHash:record.hash,measurementReason:measurement.reason});
     }catch{invalidate(p);}
    }
   }
  }
 }
 state.sourceSequence=record.sequence;state.lastRecordHash=record.hash;return {state,observations};
}
