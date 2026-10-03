import {advanceBidAnchor,bidAnchorReport} from './bid-anchor.mjs';
import {invalidate} from './model.mjs';import {instant} from '../paper/fixed.mjs';
const need=(ok)=>{if(!ok)throw Error('BID_ANCHOR_AVAILABILITY');};
function fence(s,id,at){
 const ids=id===null?new Set([...Object.keys(s.histories),...Object.values(s.wallets).flatMap(w=>Object.values(w.state.books).map(b=>b.marketId))]):new Set([id]);
 for(const market of ids){s.live.barriers[market]=at;delete s.histories[market];delete s.seen[market];}
 for(const w of Object.values(s.wallets)){invalidate(w.state,id,at,'actual_consumer_boundary');for(const [key,i]of Object.entries(w.state.strategies.mean_reversion.pending))if(i.action==='buy'&&(id===null||w.state.books[key]?.marketId===id))delete w.state.strategies.mean_reversion.pending[key];}
}
export function advanceLiveBidAnchor(s,row,bank,availableAt){
 s.live??={lastAvailableAt:bank.cutoff,barriers:{},counts:{}};
 need(instant(row.at)<=instant(availableAt)&&instant(availableAt)>=instant(s.live.lastAvailableAt));s.live.lastAvailableAt=availableAt;
 const inc=k=>s.live.counts[k]=(s.live.counts[k]||0)+1;
 if(['session_start','session_stop','invalidation'].includes(row.kind)){advanceBidAnchor(s,row,bank);fence(s,row.kind==='invalidation'?row.payload.marketId:null,availableAt);inc('source_boundary');return s;}
 if(row.kind!=='book')return s;const b=row.payload.book,id=b.marketId;
 if(instant(availableAt)-instant(b.receivedAt)>bank.config.maxDataAgeMs){if(!s.live.barriers[id])fence(s,id,availableAt);inc('stale_consumer_book');return s;}
 if(s.live.barriers[id]&&instant(b.requestStartedAt)<=instant(s.live.barriers[id])){inc('request_before_consumer_barrier');return s;}
 delete s.live.barriers[id];
 const before=Object.fromEntries(Object.entries(s.wallets).map(([key,w])=>[key,Object.fromEntries(Object.entries(w.state.strategies.mean_reversion.pending).map(([id,i])=>[id,{id:i.id,createdAt:i.createdAt}]))]));
 const priorSignals=s.signals.length;advanceBidAnchor(s,row,bank);
 for(const signal of s.signals.slice(priorSignals))signal.actualAvailableAt=availableAt;
 for(const [key,w]of Object.entries(s.wallets))for(const [instrument,i]of Object.entries(w.state.strategies.mean_reversion.pending)){
  const old=before[key][instrument];if(!old||old.id!==i.id||old.createdAt!==i.createdAt){i.createdAt=availableAt;i.actualAvailableAt=availableAt;}
 }
 inc('fresh_consumer_book');return s;
}
export function liveBidAnchorReport(s,bank,at){const r=bidAnchorReport(s,bank,at);return {...r,mode:'bid_anchor_actual_receipt_paper',updatedAt:at,actualConsumer:s.live,limitations:['Consumer availability is recorded in the original checkpoint journal; restarted backlogs do not receive retroactive entries.','Fills remain local next-request counterfactuals with one-percent depth and adverse ticks, not contest orders.','Prior executable bids are an unproven signal hypothesis.','Correlated markets and latency wallets are not independent evidence.']};}
