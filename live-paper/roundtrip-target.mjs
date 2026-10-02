import {initialState,stepForecast,report,invalidate} from './model.mjs';
import {amount,fee,TICK} from '../paper/fixed.mjs';
export const ROUND_TRIP='delayed_fifo_roundtrip_net_v1';
export const LEGACY_TARGET='legacy_decision_time_bid_forecast_not_executable_roundtrip_target';
export function targetPolicy(value=LEGACY_TARGET){if(![LEGACY_TARGET,ROUND_TRIP].includes(value))throw Error('Unsupported target policy');return value;}
// A conditional, flat-wallet counterfactual label, never an actual order or PnL claim.
export function roundTripLabel(rows,index,c,horizonMs,{includeOutcome=false,asOf=null,terminalInvalidated=false}={}){
 const r=rows[index];let s=null,lastAt=r.at;
 if(asOf!==null&&(!Number.isFinite(Date.parse(asOf))||Date.parse(asOf)<Date.parse(rows.at(-1).at)))throw Error('Invalid outcome cutoff');
 const finish=(result,at=lastAt,boundary=false)=>{
  if(!includeOutcome)return result;
  const snapshot=s?structuredClone(s):initialState(c,'diagnostic-empty',r.at);
  if(boundary||terminalInvalidated&&result.reason)invalidate(snapshot,r.marketId||r.book.marketId,at,'diagnostic_source_boundary');
  const st=snapshot.strategies.mean_reversion,v=report(snapshot,c,at).strategies.mean_reversion,buys=st.fills.filter(x=>x.action==='buy'),sells=st.fills.filter(x=>x.action==='sell');
  return {...result,outcome:{decisionAt:r.at,observedThrough:at,marketId:r.marketId||r.book.marketId,instrument:r.instrument,reason:result.reason||'completed',enteredQuantity:buys.reduce((n,x)=>n+x.quantity,0),soldQuantity:sells.reduce((n,x)=>n+x.quantity,0),openQuantity:st.positions[r.instrument]?.quantity||0,pendingAction:st.pending[r.instrument]?.action||null,cashDelta:(Number(amount(st.cash)-amount(c.initialCash))/1e6),realizedPnl:Number(amount(st.realized))/1e6,fees:Number(amount(st.fees))/1e6,referencePnl:v.referencePnl===null?null:Number(amount(v.referencePnl))/1e6,liquidationPnl:v.liquidationPnl===null?null:Number(amount(v.liquidationPnl))/1e6,fillExecutions:st.fills.length,lastObservationAt:s?.books[r.instrument]?.receivedAt||null}};
 };
if(r.historyLength<6||!Number.isInteger(r.historyLength))return finish({reason:'decision_warmup'});
 s=stepForecast(initialState(c,'training-label',r.at),r.book,c,{exitValue:1,horizonMs,minimumEdge:1e-9});
 if(!s.strategies.mean_reversion.pending[r.instrument])return finish({reason:'entry_not_admitted'});
 for(let j=index+1;j<rows.length;j++){
  const f=rows[j];if(f.instrument!==r.instrument)continue;if(f.segment!==r.segment)return finish({reason:'segment_boundary'},f.at,true);lastAt=f.at;
  s=stepForecast(s,f.book,c,{exitValue:null,horizonMs,minimumEdge:1e-9});
  const st=s.strategies.mean_reversion,buys=st.fills.filter(x=>x.action==='buy');
  if(!buys.length&&!st.pending[r.instrument])return finish({reason:'entry_unfilled'});
  if(buys.length&&!st.positions[r.instrument]&&!st.pending[r.instrument]){
   const quantity=buys.reduce((n,x)=>n+x.quantity,0),bid=amount(r.book.bids[0].price)-TICK*BigInt(c.slippageTicks),ask=amount(r.book.asks[0].price)+TICK*BigInt(c.slippageTicks),costs={feeBps:c.feeBps,feePerShare:'0.000000'};
   const persistence=Number((bid-ask)*BigInt(quantity)-fee(bid*BigInt(quantity),quantity,costs)-fee(ask*BigInt(quantity),quantity,costs))/1e6/quantity;
   return finish({label:{...r,y:Number(amount(st.realized))/1e6/quantity,persistence,labelAt:f.at,entryAt:buys[0].at,quantity,fillExecutions:st.fills.length}});
  }
 }
 return finish({reason:s.strategies.mean_reversion.positions[r.instrument]?'exit_censored':'entry_censored'},asOf||lastAt);
}
