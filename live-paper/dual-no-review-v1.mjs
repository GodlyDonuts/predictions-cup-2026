import {payloadHash} from '../execution/ledger.mjs';
import {validateCapacityReview} from './canonical-no-capacity.mjs';
import {instant} from '../paper/fixed.mjs';
const need=(ok,code)=>{if(!ok)throw Error('DUAL_NO_REVIEW_'+code);};
export function validateDualNoReview(review){
 validateCapacityReview(review);
 const fields=['raceId','stageId','usState','raceName','raceStage','officeLevel','contractType','electionDate','resolutionType'];
 const roots=review.legs.map(leg=>{const root=leg.root,p=root?.contract_details;
  need(root?.node_type==='contract'&&root.contract_type==='Election Outcome'&&root.settled_with===null&&p&&leg.captureMarketId===leg.marketId,'ROOT');
  need(typeof root.contract_id==='string'&&/^[1-9]\d*$/.test(root.contract_id)&&root.node_id===root.contract_id,'CONTRACT_ID');
  need(payloadHash(root)===leg.rulesHash,'ROOT_HASH');
  need(fields.every(k=>typeof p[k]==='string'&&p[k].length>0)&&p.raceStage==='General'&&p.resolutionType==='Party Winner'&&p.contractType==='Election Outcome','PREDICATE');
  need(['Democratic Party','Republican Party'].includes(p.winnerName),'PARTY');
  need(/^\d{4}-\d{2}-\d{2}$/.test(p.electionDate)&&instant(root.settlement_date)>instant(review.recordedAt),'SETTLEMENT');
  return root;
 });
 need(roots[0].contract_details.winnerName!==roots[1].contract_details.winnerName,'DIFFERENT_PARTIES');
 need(fields.every(k=>roots[0].contract_details[k]===roots[1].contract_details[k])&&roots[0].settlement_date===roots[1].settlement_date,'SAME_RACE_STAGE');
 need(review.normalResolutionOnly===true&&review.administrativeRefundGapAcknowledged===true,'EXCEPTION_ACK');
 // Separate party-winner markets are not exhaustive complements. Their NO/NO
 // floor follows only from normal mutual exclusion; third-party wins pay both.
 for(const pair of [[0,1],[1,0],[1,1]])need(review.scenarios.some(s=>s.category==='ordinary'&&s.payouts.every((x,i)=>x===pair[i])),'NORMAL_SCENARIOS');
 return review;
}
