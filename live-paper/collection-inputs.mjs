import {digest} from './model.mjs';
import {dipDepthPlan} from './dip-shadow.mjs';
import {historicalReference} from './history.mjs';
import {historyAllocation} from './history-allocation.mjs';

// Risk membership only: a research wallet never contributes execution leads.
export function shockCollectionRisk(state, bank, universe) {
 if(state?.bankHash!==digest(bank)||bank?.name!=='shock-rebound-shadow-v1'||bank.config?.tournamentId!==universe.tournamentId||!Array.isArray(bank.markets))throw Error('Shock collection identity mismatch');
 const official=new Map(universe.exchanges.map(x=>[x.marketId,x])),members=new Map();
 for(const m of bank.markets){const o=official.get(m.marketId);if(!o||o.exchangeId!==m.exchangeId||o.option!=='YES'||members.has(m.marketId))throw Error('Shock collection scope mismatch');members.set(m.marketId,m.exchangeId);}
 const st=state.wallet?.strategies?.mean_reversion;
 const collect=rows=>{if(!rows||typeof rows!=='object'||Array.isArray(rows))throw Error('Invalid shock holdings');return [...new Set(Object.keys(rows).map(id=>{const b=state.wallet.books?.[id],exchange=b&&members.get(b.marketId);if(!exchange||b.exchangeId!==exchange||id!==exchange+':YES')throw Error('Unmapped shock managed market');return b.marketId;}))];};
 return {heldMarketIds:collect(st?.positions),pendingMarketIds:collect(st?.pending)};
}

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
 return {heldMarketIds:[...held],pendingMarketIds:[...pending],candidateMarketIds:[...candidates],usableMarketIds:[...usable],historyAllocation:historyAllocation(sources,universe,coverage,now)};
}
