import {digest} from './model.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
export function historyEndpoint(exchangeId,at){
 if(!/^[1-9]\d*$/.test(exchangeId)||!Number.isFinite(Date.parse(at)))throw Error('Invalid history request');
 const to=new Date(Math.floor(Date.parse(at)/60000)*60000).toISOString();
 return '/exchanges/'+exchangeId+'/price-history?tournamentId='+tid+'&resolution=1m&limit=500&to='+encodeURIComponent(to);
}
export function allowedHistoryEndpoint(endpoint){try{const u=new URL('https://sig.thesuper.market/api/v1'+endpoint),id=/^\/api\/v1\/exchanges\/([1-9]\d*)\/price-history$/.exec(u.pathname)?.[1],at=u.searchParams.get('to');return Boolean(id&&at&&historyEndpoint(id,at)===endpoint);}catch{return false;}}
export function normalizeHistory(response,spec){
 const d=response.data,started=Date.parse(response.requestStartedAt),received=Date.parse(response.receivedAt),url=new URL('https://sig.thesuper.market/api/v1'+response.endpoint);
 if(!Number.isFinite(started)||!Number.isFinite(received)||received<started||received-started>20000||url.pathname!=='/api/v1/exchanges/'+spec.exchangeId+'/price-history'||url.searchParams.get('tournamentId')!==tid||url.searchParams.get('resolution')!=='1m'||url.searchParams.get('limit')!=='500'||d?.exchangeId!==spec.exchangeId||d.marketId!==spec.marketId||d.resolution!=='1m'||!Array.isArray(d.candles)||d.candles.length>500||typeof d.coverage?.complete!=='boolean'||!(d.coverage.projectedThroughSequence===null||Number.isSafeInteger(d.coverage.projectedThroughSequence)&&d.coverage.projectedThroughSequence>=0))throw Error('Invalid historical response scope');
 const from=Date.parse(d.from),to=Date.parse(d.to),requestedTo=Date.parse(url.searchParams.get('to'));if(!Number.isFinite(from)||!Number.isFinite(to)||to<=from||to>requestedTo||to>started||to%60000)throw Error('Invalid historical window');
 const seen=new Set(),candles=[];
 for(const c of d.candles){const at=Date.parse(c.time);if(!Number.isFinite(at)||at%60000||at<from||at+60000>to||seen.has(at)||!Number.isFinite(c.volume)||c.volume<0||!Number.isSafeInteger(c.tradeCount)||c.tradeCount<0)throw Error('Invalid candle clock/count');seen.add(at);
  for(const k of ['open','high','low','close','vwap'])if(c[k]!==null&&(!Number.isFinite(c[k])||c[k]<0||c[k]>1))throw Error('Invalid candle price');
  if(c.volume>0&&c.tradeCount>0){if(['open','high','low','close','vwap'].some(k=>c[k]===null)||c.low>c.high||[c.open,c.close,c.vwap].some(x=>x<c.low-1e-12||x>c.high+1e-12))throw Error('Invalid OHLC bounds');candles.push({...c,time:new Date(at).toISOString()});}
 }
 candles.sort((a,b)=>Date.parse(a.time)-Date.parse(b.time));return {exchangeId:spec.exchangeId,marketId:spec.marketId,tournamentId:tid,resolution:'1m',from:d.from,to:d.to,receivedAt:response.receivedAt,requestStartedAt:response.requestStartedAt,coverage:structuredClone(d.coverage),candles,source:'official_tournament_trade_candles',sparse:true,executable:false,hash:digest({endpoint:response.endpoint,data:d})};
}
const median=a=>{const x=a.slice().sort((a,b)=>a-b),n=x.length;return n%2?x[(n-1)/2]:(x[n/2-1]+x[n/2])/2;};
export function historicalReference(history,books,at,p){
 const now=Date.parse(at);if(!Number.isFinite(now))throw Error('Invalid reference clock');
 let points=[],source;
 if(history&&Date.parse(history.receivedAt)<=now&&now-Date.parse(history.receivedAt)<=p.historyCacheAgeMs&&history.coverage.complete){
  points=history.candles.filter(c=>Date.parse(c.time)+60000<=now&&Date.parse(c.time)>=now-p.lookbackMs).map(c=>({at:Date.parse(c.time)+60000,price:c.vwap}));source='closed_trade_candle_vwap_median';
 }
 if(points.length<p.minimumSamples){points=(books||[]).filter(b=>Date.parse(b.at)<now&&Date.parse(b.receivedAt)<=now&&Date.parse(b.at)>=now-p.lookbackMs).map(b=>({at:Date.parse(b.at),price:b.mid}));source='prior_verified_orderbook_midpoint_median';}
 if(points.length<p.minimumSamples)return {ready:false,reason:'insufficient_history',samples:points.length};
 const first=Math.min(...points.map(x=>x.at)),last=Math.max(...points.map(x=>x.at));if(last-first<p.minimumSpanMs)return {ready:false,reason:'insufficient_history_span',samples:points.length};if(now-last>p.lastHistoricalObservationAgeMs)return {ready:false,reason:'historical_reference_stale',samples:points.length};
 const center=median(points.map(x=>x.price)),mad=median(points.map(x=>Math.abs(x.price-center)));
 return {ready:true,source,samples:points.length,firstAt:new Date(first).toISOString(),lastAt:new Date(last).toISOString(),center,robustDeviation:Math.max(.0025,mad*1.4826)};
}
