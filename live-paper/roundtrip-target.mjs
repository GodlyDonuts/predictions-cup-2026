import {initialState,stepForecast} from './model.mjs';
import {amount,fee,TICK} from '../paper/fixed.mjs';
export const ROUND_TRIP='delayed_fifo_roundtrip_net_v1';
export const LEGACY_TARGET='legacy_decision_time_bid_forecast_not_executable_roundtrip_target';
export function targetPolicy(value=LEGACY_TARGET){if(![LEGACY_TARGET,ROUND_TRIP].includes(value))throw Error('Unsupported target policy');return value;}
// A conditional, flat-wallet counterfactual label, never an actual order or PnL claim.
export function roundTripLabel(rows,index,c,horizonMs){
 const r=rows[index];if(r.historyLength<6||!Number.isInteger(r.historyLength))return {reason:'decision_warmup'};
 let s=stepForecast(initialState(c,'training-label',r.at),r.book,c,{exitValue:1,horizonMs,minimumEdge:1e-9});
 if(!s.strategies.mean_reversion.pending[r.instrument])return {reason:'entry_not_admitted'};
 for(let j=index+1;j<rows.length;j++){
  const f=rows[j];if(f.instrument!==r.instrument)continue;if(f.segment!==r.segment)return {reason:'segment_boundary'};
  s=stepForecast(s,f.book,c,{exitValue:null,horizonMs,minimumEdge:1e-9});
  const st=s.strategies.mean_reversion,buys=st.fills.filter(x=>x.action==='buy');
  if(!buys.length&&!st.pending[r.instrument])return {reason:'entry_unfilled'};
  if(buys.length&&!st.positions[r.instrument]&&!st.pending[r.instrument]){
   const quantity=buys.reduce((n,x)=>n+x.quantity,0),bid=amount(r.book.bids[0].price)-TICK*BigInt(c.slippageTicks),ask=amount(r.book.asks[0].price)+TICK*BigInt(c.slippageTicks),costs={feeBps:c.feeBps,feePerShare:'0.000000'};
   const persistence=Number((bid-ask)*BigInt(quantity)-fee(bid*BigInt(quantity),quantity,costs)-fee(ask*BigInt(quantity),quantity,costs))/1e6/quantity;
   return {label:{...r,y:Number(amount(st.realized))/1e6/quantity,persistence,labelAt:f.at,entryAt:buys[0].at,quantity,fillExecutions:st.fills.length}};
  }
 }
 return {reason:s.strategies.mean_reversion.positions[r.instrument]?'exit_censored':'entry_censored'};
}
