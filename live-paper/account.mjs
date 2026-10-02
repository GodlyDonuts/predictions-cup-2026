const tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x);
const number=x=>typeof x==='number'&&Number.isFinite(x);
const timestamp=x=>typeof x==='string'&&Number.isFinite(Date.parse(x));
const title=x=>typeof x==='string'?x.slice(0,240):null;
export function accountPositions(response){
 if(!Array.isArray(response.data?.positions)||!timestamp(response.receivedAt))throw Error('Invalid account positions');
 return {at:response.receivedAt,rows:response.data.positions.filter(x=>x.quantity!==0).map(x=>{
  if(!id(x.exchangeId)||!id(x.marketId)||!Number.isSafeInteger(x.quantity)||!['avgCost','marketValue','costBasis','unrealizedPnl'].every(k=>number(x[k]))||!number(x.currentPrice))throw Error('Invalid account position');
  return {instrument:x.exchangeId+':'+(x.quantity<0?'NO':'YES'),exchangeId:x.exchangeId,marketId:x.marketId,label:title(x.marketTitle),side:x.quantity<0?'no':'yes',quantity:Math.abs(x.quantity),avgCost:x.avgCost,cost:x.costBasis,valuationPrice:x.currentPrice,valuationValue:x.marketValue,unrealizedPnl:x.unrealizedPnl};
 })};
}
export function accountOrders(rows,at,{openOnly=true}={}){
 if(!Array.isArray(rows)||!timestamp(at))throw Error('Invalid account orders');
 return {at,rows:rows.map(x=>{
  if(!Number.isSafeInteger(x.id)||x.id<=0||!id(x.exchangeId)||x.tournamentId!==tid||!['yes','no'].includes(x.side)||!['buy','sell'].includes(x.action)||!number(x.quantity)||x.quantity<0||typeof x.open!=='boolean'||openOnly&&!x.open||x.priceLimit!==null&&!number(x.priceLimit)||!timestamp(x.createdAt))throw Error('Invalid account order');
  return {id:x.id,instrument:x.exchangeId+':'+x.side.toUpperCase(),exchangeId:x.exchangeId,side:x.side,action:x.action,quantity:x.quantity,limit:x.priceLimit,createdAt:x.createdAt,open:x.open};
 })};
}
export function accountFills(response){
 const d=response.data;if(!Array.isArray(d?.data)||d.data.length>100||typeof d.pagination?.hasMore!=='boolean'||d.coverage?.complete!==true||!timestamp(response.receivedAt))throw Error('Invalid account fills');
 return {at:response.receivedAt,hasMore:d.pagination.hasMore,rows:d.data.map(x=>{
  if(!Number.isSafeInteger(x.id)||x.id<=0||x.orderId!==null&&!Number.isSafeInteger(x.orderId)||!id(x.exchangeId)||!id(x.marketId)||!number(x.quantity)||!x.quantity||!['yes','no'].includes(x.side)||x.side==='no'&&x.quantity>0||x.side==='yes'&&x.quantity<0||x.price!==null&&!number(x.price)||!timestamp(x.filledAt))throw Error('Invalid account fill');
  return {id:x.id,orderId:x.orderId,instrument:x.exchangeId+':'+x.side.toUpperCase(),exchangeId:x.exchangeId,marketId:x.marketId,side:x.side,quantity:Math.abs(x.quantity),price:x.price,at:x.filledAt};
 })};
}
export function accountPnl(response){
 const d=response.data;if(d?.period!=='day'||!['totalAccountValue','totalHoldingsValue','totalCostBasis','unrealizedPnl'].every(k=>number(d[k]))||d.periodPnl!==null&&!number(d.periodPnl)||!timestamp(response.receivedAt))throw Error('Invalid account valuation');
 return {at:response.receivedAt,accountValue:d.totalAccountValue,holdingsValue:d.totalHoldingsValue,costBasis:d.totalCostBasis,unrealizedPnl:d.unrealizedPnl,periodPnl:d.periodPnl,period:'day'};
}
export function accountView(cache,collectorStatus,now,identities={}){
 const ages=Object.fromEntries(['positions','orders','fills','pnl'].map(k=>[k,cache[k]?.at?Math.max(0,Date.parse(now)-Date.parse(cache[k].at)):null]));
 const labels=new Map((cache.positions?.rows??[]).map(x=>[x.exchangeId,x.label]));
 const label=x=>labels.get(x.exchangeId)||identities[x.marketId]?.title||'Exchange '+x.exchangeId;
 const orderMap=new Map([...(cache.orderHistory?.rows??[]),...(cache.orders?.rows??[])].map(x=>[x.id,x]));
 return {mode:'official_account',available:!!(cache.positions||cache.fills||cache.pnl),updatedAt:now,collectorStatus,ages,sourceTimes:Object.fromEntries(['positions','orders','fills','pnl'].map(k=>[k,cache[k]?.at??null])),positions:(cache.positions?.rows??[]).map(x=>({...x,label:label(x)})),pending:(cache.orders?.rows??[]).map(x=>({...x,label:label(x)})),trades:(cache.fills?.rows??[]).map(x=>({...x,label:label(x),action:orderMap.get(x.orderId)?.action??null})),metrics:cache.pnl??null,fillHistory:{displayed:cache.fills?.rows.length??0,hasMore:cache.fills?.hasMore??null},errors:cache.errors??{}};
}
