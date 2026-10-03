import {amount,money,TICK,instant} from '../paper/fixed.mjs';
import {BID_ANCHOR_POLICY,createBidAnchorState} from './bid-anchor.mjs';
import {advanceLiveBidAnchor,liveBidAnchorReport} from './bid-anchor-live.mjs';
import {validatePriceObservation} from './market-price-publication.mjs';
import {validateUniverse} from './breadth.mjs';
const need=ok=>{if(!ok)throw Error('BULK_DIP_INVALID');},price=n=>money(BigInt(Math.round(n*1e6)));
export const BULK_DIP_POLICY=Object.freeze({reference:'bulk_quoted_bid_not_executable',candidateLifetimeMs:60000,maximumReceiptAgeMs:20000});
export function createBulkDipState(bank){return {...createBidAnchorState(bank),bulk:{history:{},candidates:{},counts:{},lastAvailableAt:bank.cutoff}};}
export function quotedDipCandidate(points,q,at,p=BID_ANCHOR_POLICY){
 const now=instant(at),a=points.filter(x=>instant(x.at)<now&&instant(x.at)>=now-p.historyMs);
 if(a.length<p.minimumMinutes||instant(a.at(-1).at)-instant(a[0].at)<p.minimumSpanMs||now-instant(a.at(-1).at)>p.maximumGapMs||a.some((x,i)=>i&&instant(x.at)-instant(a[i-1].at)>p.maximumGapMs))return null;
 if(q.crossed||typeof q.bestBid!=='number'||typeof q.bestAsk!=='number'||q.bestBid<=0||q.bestAsk>=1||q.bestBid>=q.bestAsk)return null;
 const values=a.map(x=>amount(x.bid)).sort((x,y)=>x<y?-1:x>y?1:0),anchor=values[Math.floor((values.length-1)/2)],bid=amount(price(q.bestBid)),ask=amount(price(q.bestAsk)),concession=anchor-2n*TICK-ask;
 if(values.at(-1)-values[0]>amount(p.maximumAnchorRange)||ask-bid>amount(p.maximumSpread)||anchor-bid>amount(p.maximumBidDrop)||concession<amount(p.minimumNetConcession))return null;
 return {marketId:q.marketId,exchangeId:q.exchangeId,anchor:money(anchor),concession:money(concession),maximumAsk:money(anchor-2n*TICK-amount(p.minimumNetConcession)),referenceKind:'bulk_quoted_bid_not_executable',depthVerified:false};
}
export function advanceBulkPrices(s,snapshot,bank,availableAt,{warmup=false}={}){
 validatePriceObservation(snapshot,validateUniverse(bank.universe,bank.config.tournamentId));const at=instant(availableAt);need(instant(snapshot.receivedAt)<=at&&at>=instant(s.bulk.lastAvailableAt));s.bulk.lastAvailableAt=availableAt;
 const inc=k=>s.bulk.counts[k]=(s.bulk.counts[k]||0)+1,byExchange=new Map(bank.universe.exchanges.map(x=>[x.exchangeId,x.marketId]));
 if(!warmup&&at-instant(snapshot.receivedAt)>bank.bulkPolicy.maximumReceiptAgeMs){inc('stale_price_receipt');s.bulk.candidates={};return s;}
 for(const id of snapshot.missingIds){const market=byExchange.get(id);delete s.bulk.history[market];delete s.bulk.candidates[market];inc('missing_price_market');}
 for(const q of snapshot.quotes){const id=q.marketId;if(q.option!=='YES'||q.crossed||q.bestBid===null||q.bestAsk===null||q.bestBid<=0||q.bestAsk>=1||q.bestBid>=q.bestAsk){delete s.bulk.history[id];delete s.bulk.candidates[id];inc('invalid_reference');continue;}
  const points=s.bulk.history[id]||[],prior=s.bulk.candidates[id],candidate=warmup?null:quotedDipCandidate(points,q,snapshot.receivedAt,bank.policy);delete s.bulk.candidates[id];
  if(candidate){s.bulk.candidates[id]={...candidate,referencePoints:points.filter(x=>instant(x.at)<instant(snapshot.receivedAt)),observedAt:snapshot.receivedAt,availableAt,expiresAt:new Date(at+bank.bulkPolicy.candidateLifetimeMs).toISOString()};inc('depth_candidate');}
  else if(!warmup&&prior&&at<=instant(prior.expiresAt)&&amount(price(q.bestAsk))<=amount(prior.maximumAsk)&&amount(prior.anchor)-amount(price(q.bestBid))<=amount(bank.policy.maximumBidDrop)&&amount(price(q.bestAsk))-amount(price(q.bestBid))<=amount(bank.policy.maximumSpread))s.bulk.candidates[id]=prior;
  const minute=Math.floor(instant(snapshot.receivedAt)/60000);if(!points.some(x=>x.minute===minute))points.push({minute,at:snapshot.receivedAt,bid:price(q.bestBid)});
  s.bulk.history[id]=points.filter(x=>instant(x.at)>=instant(snapshot.receivedAt)-bank.policy.historyMs);inc(warmup?'warmup_reference':'fresh_reference');
 }
 return s;
}
export function advanceBulkBook(s,row,bank,availableAt){
 need(instant(availableAt)>=instant(s.bulk.lastAvailableAt));
 if(['session_start','session_stop'].includes(row.kind)||row.kind==='invalidation'&&row.payload.marketId===null){s.bulk.history={};s.bulk.candidates={};}
 else if(row.kind==='invalidation'){delete s.bulk.history[row.payload.marketId];delete s.bulk.candidates[row.payload.marketId];}
 if(row.kind==='book'){
  const b=row.payload.book;
  // References predate the depth request. The engine still verifies actual
  // spread/size and cannot fill a new intent on this same observation.
  const candidate=s.bulk.candidates[b.marketId],points=candidate&&instant(candidate.expiresAt)>=instant(availableAt)?candidate.referencePoints:s.bulk.history[b.marketId]||[];
  s.histories={[b.marketId]:points.filter(x=>instant(x.at)<instant(b.requestStartedAt))};
 }
 advanceLiveBidAnchor(s,row,bank,availableAt);return s;
}
export function bulkDipReport(s,bank,at){return {...liveBidAnchorReport(s,bank,at),mode:'market_wide_bulk_reference_paper',bulkReference:{marketCount:Object.keys(s.bulk.history).length,counts:s.bulk.counts,candidates:Object.values(s.bulk.candidates).filter(x=>instant(x.expiresAt)>=instant(at))},limitations:['Bulk prices supply quoted historical reference only; their source quote age and executable size are unknown.','Every fill requires later independent verified depth, actual consumer receipt, original limit, adverse ticks and one-percent participation.','Two latency wallets are correlated local counterfactuals, not contest orders or profit evidence.']};}
