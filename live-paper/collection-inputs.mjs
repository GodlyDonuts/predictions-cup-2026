import {digest} from './model.mjs';
import {dipDepthPlan} from './dip-shadow.mjs';
import {historicalReference} from './history.mjs';

// A stopped worker may still hold inventory; keep servicing it while suppressing
// new candidates. Source identity mismatches must not silently hide positions.
export function collectionInputs(sources, universe, coverage, now) {
 const valid=new Set(universe.exchanges.map(x=>x.marketId)), held=new Set(), pending=new Set(), candidates=new Set(), usable=new Set();
 for(const {state,bank,healthy} of sources){
  if(state.bankHash!==digest(bank)||bank.universe.hash!==universe.hash)throw Error('Collection worker identity mismatch');
  const st=state.wallet.strategies.mean_reversion;
  for(const [rows,target] of [[st.positions,held],[st.pending,pending]])for(const id of Object.keys(rows)){
   const market=state.wallet.books[id]?.marketId;if(!valid.has(market))throw Error('Unmapped managed market');target.add(market);
  }
  if(!healthy)continue;
  for(const lead of dipDepthPlan(state,bank,coverage,now))if(lead.reason==='historical_discount_lead')candidates.add(lead.marketId);
  for(const spec of universe.exchanges)if(historicalReference(state.histories[spec.exchangeId],state.bookHistory[spec.exchangeId],new Date(now).toISOString(),bank.policy).ready)usable.add(spec.marketId);
 }
 return {heldMarketIds:[...held],pendingMarketIds:[...pending],candidateMarketIds:[...candidates],usableMarketIds:[...usable]};
}
