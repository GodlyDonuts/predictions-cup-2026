import fs from 'node:fs';import path from 'node:path';import {pathToFileURL} from 'node:url';
import {payloadHash} from '../../execution/ledger.mjs';import {amount,money} from '../../paper/fixed.mjs';
import {instantNs} from '../../feed/contract.mjs';

export function streamActivity(rows,referenceAt,pairs){
 const known=new Map(pairs.map(p=>[p.exchangeId,p.marketId])),markets=new Map(),seen=new Map(),brokenMarkets=new Set(),at=instantNs(referenceAt);
 for(const row of rows){const e=row.event;if(e.kind!=='broadcast'||e.event!=='market_batch')continue;
  if(e.source!=='official_sig_realtime')throw Error('ACTIVITY_SOURCE');
  if(e.gap||e.payload.resyncRequired)brokenMarkets.add(e.marketId);
  for(const book of e.payload.books){
   const id=String(book.exchangeId);if(known.get(id)!==e.marketId)throw Error('ACTIVITY_SCOPE');
   let m=markets.get(id);if(!m){m={marketId:e.marketId,exchangeId:id,trades:[],book:null,gaps:0};markets.set(id,m);}
   if(!m.book||instantNs(book.asOf.at)>instantNs(m.book.asOf.at))m.book=book;
  }
  if(e.duplicate||e.gap||e.payload.resyncRequired)continue;
  for(const trade of e.payload.trades){
   const id=String(trade.exchangeId);if(known.get(id)!==e.marketId)throw Error('ACTIVITY_TRADE_SCOPE');
   if(trade.id===null||trade.sequence===null)continue;
   const t=instantNs(trade.executedAt);if(t>instantNs(e.receivedAt)||t>at||!Number.isSafeInteger(trade.quantity)||trade.quantity<=0)throw Error('ACTIVITY_TRADE_FACT');
   const key=id+':'+trade.id,hash=payloadHash(trade);if(seen.has(key)){if(seen.get(key)!==hash)throw Error('ACTIVITY_TRADE_CONFLICT');continue;}seen.set(key,hash);
   let m=markets.get(id);if(!m){m={marketId:e.marketId,exchangeId:id,trades:[],book:null,gaps:0};markets.set(id,m);}
   m.trades.push({...trade,receivedAt:e.receivedAt});
  }
 }
 return [...markets.values()].map(m=>{
  const recent=m.trades.filter(t=>at-instantNs(t.executedAt)<=180000000000n),b=m.book;
  const bids=b?.bids??[],asks=b?.asks??[];
  const bid=bids.length?bids.reduce((a,l)=>amount(String(l.price))>amount(String(a.price))?l:a):null,ask=asks.length?asks.reduce((a,l)=>amount(String(l.price))<amount(String(a.price))?l:a):null;
  const volume=recent.reduce((n,t)=>n+BigInt(t.quantity),0n),age=b?Number(at-instantNs(b.asOf.at))/1e9:null;
  const spread=bid&&ask?amount(String(ask.price))-amount(String(bid.price)):null;
  const gaps=brokenMarkets.has(m.marketId)?1:0;
  const eligible=age!==null&&age>=0&&age<=10&&spread!==null&&spread>0n&&gaps===0;
  const lag=recent.map(t=>Number(instantNs(t.receivedAt)-instantNs(t.executedAt))/1e9);
  return{marketId:m.marketId,exchangeId:m.exchangeId,observedTrades:m.trades.length,recentTrades:recent.length,recentShares:volume.toString(),bookSourceAgeSeconds:age,
   bid:bid?.price??null,ask:ask?.price??null,spread:spread===null?null:money(spread),bidQueue:bid?.quantity??null,askQueue:ask?.quantity??null,gaps,eligible,
   meanObservedTradeLagSeconds:lag.length?lag.reduce((a,b)=>a+b,0)/lag.length:null,
   activityScore:eligible&&recent.length?Number(volume)/(Number(bid.quantity)+Number(ask.quantity)):null};
 }).sort((a,b)=>(b.activityScore??-1)-(a.activityScore??-1)||a.exchangeId.localeCompare(b.exchangeId));
}

if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
 const [directory,output]=process.argv.slice(2);if(!output)throw Error('Usage: stream-directory new-output');
 const read=file=>JSON.parse(fs.readFileSync(path.join(directory,file),'utf8')),stats=read('stats.json'),manifest=read('manifest.json');
 const rows=fs.readFileSync(path.join(directory,'events.jsonl'),'utf8').split('\n').slice(0,stats.sequence).map(JSON.parse);
 let previous='',sequence=0;for(const row of rows){const {hash,...body}=row;if(hash!==payloadHash(body)||body.previous!==previous||body.sequence!==++sequence)throw Error('ACTIVITY_JOURNAL');previous=hash;}
 if(sequence!==stats.sequence||previous!==stats.previous)throw Error('ACTIVITY_PINNED_PREFIX');
 const result={schemaVersion:1,at:stats.at,sourceSequence:sequence,sourcePrevious:previous,streamStatus:stats.status,candidates:streamActivity(rows,stats.at,manifest.pairs),contestOrders:0,profitabilityEstablished:false,
  limitations:['Observed activity ranking, not a complete trade-window guarantee or profitable signal.','Past trade volume and current displayed queues do not establish fill probability or aggressor direction.','Fresh scoped REST reconciliation and a new preregistered funded forward trial are still required.']};
 fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({sourceSequence:sequence,candidates:result.candidates.slice(0,8)},null,2));
}
