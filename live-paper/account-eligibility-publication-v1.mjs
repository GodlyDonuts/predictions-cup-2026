// Private sanitized eligibility evidence. No balances, sizes, orders or keys.
import {Journal} from './runtime.mjs';
const need=(x,c)=>{if(!x)throw Error('ACCOUNT_ELIGIBILITY_'+c);};
const clock=s=>{const n=Date.parse(s);need(Number.isSafeInteger(n)&&new Date(n).toISOString()===s,'CLOCK');return n;};
export function eligibilitySnapshot({positions,orders,ordersAt,universe}){
 need(Array.isArray(positions?.data?.positions)&&Array.isArray(orders),'INCOMPLETE');
 const p=clock(positions.receivedAt),o=clock(ordersAt),members=new Map(universe.exchanges.map(x=>[x.exchangeId,x.marketId])),excluded=new Set();
 need(o>=p,'CLOCK');
 for(const [rows,isOrder] of [[positions.data.positions,false],[orders,true]])for(const row of rows){
  need(row&&typeof row.exchangeId==='string'&&Number.isSafeInteger(row.quantity)&&(!isOrder||row.quantity>0),'ROW');
  if(!isOrder&&row.quantity===0)continue;
  const market=members.get(row.exchangeId);need(market&&(!row.marketId||row.marketId===market),'SCOPE');excluded.add(market);
 }
 return {tournamentId:universe.tournamentId,universeHash:universe.hash,positionsAt:positions.receivedAt,ordersAt,
  expiresAt:new Date(Math.min(p,o)+20000).toISOString(),excludedMarketIds:[...excluded].sort((a,b)=>Number(a)-Number(b)),complete:true};
}
export function createAccountEligibilityPublication({file,universe,canOperate=()=>true}){
 const journal=new Journal(file);if(journal.last)need(journal.last.state===null&&journal.last.event?.type==='market_account_eligibility'&&journal.last.event.snapshot.universeHash===universe.hash,'SCOPE_CHANGED');
 return {publish(input){need(canOperate(),'STOPPED');const snapshot=eligibilitySnapshot({...input,universe});
  if(journal.last)need(clock(snapshot.positionsAt)>=clock(journal.last.event.snapshot.positionsAt)&&clock(snapshot.ordersAt)>=clock(journal.last.event.snapshot.ordersAt),'ROLLBACK');
  journal.commit(null,{type:'market_account_eligibility',snapshot});return{sequence:journal.sequence,expiresAt:snapshot.expiresAt};},
  status:()=>({sequence:journal.sequence,hash:journal.previous,networkRequests:0,competitionOrders:0})};
}
