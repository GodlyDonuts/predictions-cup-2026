import {amount,TICK,instant,sequence} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';

export const RECOVERY_CONFIRMATION_POLICY=Object.freeze({windowMs:180000,minimumObservations:4,
 minimumSpacingMs:1000,maximumGapMs:90000,minimumDropTicks:2,minimumReboundTicks:1,minimumAfterTrough:2});

// These are point observations across verified recoveries, not a complete tape.
// A temporary input invalidation does not retroactively falsify an earlier point.
export function observeRecovery(points,event,policy=RECOVERY_CONFIRMATION_POLICY){
 const book=event.book,at=instant(event.quoteReceivedAt),prior=points.at(-1);
 if(!event.decisionEligible||!event.continuity?.ready||!event.continuity?.continuous)return points;
 if(prior&&(sequence(book.sequence)<=sequence(prior.sequence)||instantNs(book.sourceAt)<=instantNs(prior.sourceAt)))return points;
 if(prior&&at<=instant(prior.receivedAt))return points;
 const next={receivedAt:event.quoteReceivedAt,confirmedAt:event.confirmedAt,availableAt:event.availableAt,
 sourceAt:book.sourceAt,sequence:book.sequence,bid:book.bids[0].price,ask:book.asks[0].price};
 return [...points,next].filter(x=>instant(x.receivedAt)>=at-policy.windowMs).slice(-256);
}
export function recoveryConfirmation(points,event,at,policy=RECOVERY_CONFIRMATION_POLICY){
 const now=instant(at),current=event.book;
 const eligible=points.filter(x=>instant(x.receivedAt)<=now&&instant(x.confirmedAt)<=now&&instant(x.availableAt)<=now
   &&instant(x.receivedAt)>=now-policy.windowMs);
 // Collapse receipts that are too close; use the first observation in each
 // spacing interval so burst intensity cannot manufacture confirmation.
 const a=[];for(const x of eligible)if(!a.length||instant(x.receivedAt)-instant(a.at(-1).receivedAt)>=policy.minimumSpacingMs)a.push(x);
 const reject=reason=>({admitted:false,reason,observations:a.length});
 if(a.length<policy.minimumObservations)return reject('recovery_insufficient_observations');
 const last=a.at(-1);if(last.sequence!==current.sequence||last.sourceAt!==current.sourceAt)return reject('recovery_current_not_independent');
 for(let i=1;i<a.length;i++)if(instant(a[i].receivedAt)-instant(a[i-1].receivedAt)>policy.maximumGapMs)return reject('recovery_observation_gap');
 let trough=0;for(let i=1;i<a.length;i++)if(amount(a[i].bid)<amount(a[trough].bid))trough=i;
 if(trough===0)return reject('recovery_no_prior_drop');
 if(a.length-1-trough<policy.minimumAfterTrough)return reject('recovery_insufficient_after_trough');
 const low=amount(a[trough].bid),peak=a.slice(0,trough).reduce((p,x)=>amount(x.bid)>p?amount(x.bid):p,0n),bid=amount(last.bid);
 if(peak-low<TICK*BigInt(policy.minimumDropTicks))return reject('recovery_drop_too_small');
 if(bid-low<TICK*BigInt(policy.minimumReboundTicks))return reject('recovery_rebound_too_small');
 if(amount(a.at(-2).bid)>bid)return reject('recovery_bid_falling');
 return {admitted:true,reason:'recovery_confirmed',observations:a.length,troughReceivedAt:a[trough].receivedAt,
  dropTicks:Number((peak-low)/TICK),reboundTicks:Number((bid-low)/TICK),observedSpanMs:instant(last.receivedAt)-instant(a[0].receivedAt),
  evidence:'discrete_verified_bid_observations_not_continuous_tape_or_forecast'};
}
