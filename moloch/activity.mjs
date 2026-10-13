import fs from 'node:fs';import path from 'node:path';import {pathToFileURL} from 'node:url';import {createHash} from 'node:crypto';import {payloadHash} from '../execution/ledger.mjs';import {compareVersion} from '../feed/contract.mjs';
const check=(ok,code)=>{if(!ok)throw Error('ACTIVITY_'+code);};
const ticks=p=>Math.round(p*200);
export class ActivityStudy {
  constructor(){this.markets=new Map();this.seen=new Set();this.sequence=0;this.previous='';this.firstAt=null;this.lastAt=null;this.unknownMarkets=0;}
  market(id){let m=this.markets.get(id);if(!m){m={marketId:id,books:new Map(),flow:[],pending:[],markouts:[],gapped:false,batches:0,trades:0,volume:0};this.markets.set(id,m);}return m;}
  row(row){const {hash,...body}=row;check(hash===payloadHash(body)&&body.sequence===this.sequence+1&&body.previous===this.previous,'CHAIN');this.sequence++;this.previous=hash;const e=body.event;if(e.receivedAt){this.firstAt??=e.receivedAt;this.lastAt=e.receivedAt;}if(e.kind==='unknown_market'){this.unknownMarkets++;return;}
    if(e.kind!=='broadcast')return;const m=this.market(e.marketId);if(e.event!=='market_batch'){if(e.event==='market_settled')m.gapped=true;return;}const p=e.payload;m.batches++;
    if(e.gap||p.resyncRequired||p.trades.some(t=>t.sequence===null))m.gapped=true;
    if(!e.duplicate&&!e.gap&&!p.resyncRequired)for(const t of p.trades){if(t.id===null)continue;const key=[t.tournamentId,t.exchangeId,t.id].join(':');if(this.seen.has(key))continue;this.seen.add(key);m.trades++;m.volume+=t.quantity;m.flow.push({exchangeId:t.exchangeId,at:Date.parse(t.executedAt),quantity:t.quantity});
      const prior=m.books.get(t.exchangeId),executed=Date.parse(t.executedAt);if(!prior||m.gapped||prior.book.asOf.sequence>=t.sequence||Date.parse(prior.book.asOf.at)>executed||Date.parse(prior.receivedAt)>executed||executed-Date.parse(prior.receivedAt)>10000||executed-prior.priceSinceMs<3000)continue;
      const bid=prior.book.bids[0],ask=prior.book.asks[0];if(!bid||!ask)continue;const side=t.price<=bid.price?'YES':t.price>=ask.price?'NO':null;
      if(side)m.pending.push({tradeId:t.id,exchangeId:t.exchangeId,side,quantity:t.quantity,executedAt:t.executedAt,dueAt:executed+30000,sequence:t.sequence,quoteTicks:ticks(side==='YES'?bid.price:ask.price),touch:t.price===(side==='YES'?bid.price:ask.price)});
    }
    for(const book of p.books){const old=m.books.get(book.exchangeId);if(old&&compareVersion(book.asOf,old.book.asOf)<=0)continue;
      const bid=book.bids[0],ask=book.asks[0];if(!bid||!ask)continue;
      const unchanged=old&&old.book.bids[0]?.price===bid.price&&old.book.asks[0]?.price===ask.price;
      m.books.set(book.exchangeId,{book,receivedAt:e.receivedAt,priceSinceMs:unchanged?old.priceSinceMs:Date.parse(e.receivedAt)});
      const remaining=[];for(const sample of m.pending){if(sample.exchangeId!==book.exchangeId||Date.parse(e.receivedAt)<sample.dueAt||Date.parse(book.asOf.at)<sample.dueAt||book.asOf.sequence<sample.sequence){remaining.push(sample);continue;}
        if(m.gapped)continue;const exit=sample.side==='YES'?bid:ask;if(exit.quantity<1)continue;const pnlTicks=sample.side==='YES'?ticks(exit.price)-sample.quoteTicks:sample.quoteTicks-ticks(exit.price);
        m.markouts.push({...sample,markedAt:e.receivedAt,liquidationDeltaPerShare:(pnlTicks/200).toFixed(6),assumption:'Conditional prior-observable join quote, not a verified fill; future executable one-share mark, zero fees.'});
      }m.pending=remaining;
    }
  }
  report(endAt=this.lastAt){const end=Date.parse(endAt);return[...this.markets.values()].flatMap(m=>[...m.books.values()].map(b=>{
    const bid=b.book.bids[0],ask=b.book.asks[0],ex=b.book.exchangeId,allFlow=m.flow.filter(t=>t.exchangeId===ex),flow=allFlow.filter(t=>end-t.at>=0&&end-t.at<=180000),volume=flow.reduce((n,t)=>n+t.quantity,0),spreadTicks=ticks(ask.price)-ticks(bid.price),bookAgeMs=end-Date.parse(b.book.asOf.at),marks=m.markouts.filter(t=>t.exchangeId===ex),average=marks.length?marks.reduce((n,t)=>n+Number(t.liquidationDeltaPerShare),0)/marks.length:null;
    const eligible=!m.gapped&&volume>0&&spreadTicks>0&&bookAgeMs>=0&&bookAgeMs<=10000;
    return{marketId:m.marketId,exchangeId:ex,observedTrades:allFlow.length,observedVolume:allFlow.reduce((n,t)=>n+t.quantity,0),recentTrades:flow.length,recentVolume:volume,bid:bid.price,ask:ask.price,spreadTicks,bidQueue:bid.quantity,askQueue:ask.quantity,bookAsOf:b.book.asOf,bookReceivedAt:b.receivedAt,bookAgeMs,gapObserved:m.gapped,eligibleForFurtherReconciliation:eligible,explorationScore:eligible?spreadTicks*Math.log1p(volume/Math.min(bid.quantity,ask.quantity))*Math.log1p(flow.length):null,conditionalMarkouts:marks.length,meanConditionalLiquidationDeltaPerShare:average,censoredMarkouts:m.pending.filter(t=>t.exchangeId===ex).length};
  })).sort((a,b)=>(b.explorationScore??-1)-(a.explorationScore??-1));}
}
export function study(file,endAt=null){const s=new ActivityStudy();for(const line of fs.readFileSync(file,'utf8').trim().split(/\r?\n/).filter(Boolean))s.row(JSON.parse(line));return{schemaVersion:1,bot:'Moloch',sourceSha256:createHash('sha256').update(fs.readFileSync(file)).digest('hex'),sourceEventCount:s.sequence,sourceFinalHash:s.previous,window:{from:s.firstAt,to:endAt??s.lastAt},observedMarkets:s.markets.size,unknownCatalogMarkets:s.unknownMarkets,candidates:s.report(endAt??s.lastAt),conditionalMarkouts:[...s.markets.values()].flatMap(m=>m.markouts),contestOrders:0,paperOrders:0,profitabilityEstablished:false,limitations:['Observed flow is a lower bound from best-effort broadcasts, not a complete tournament tape.','Ranking uses current book and last-three-minute observed flow; reconcile candidates over REST before any paper admission.','Conditional markouts require a prior observable book, three-second price-regime lead and a later thirty-second executable one-share book; they are not filled-wallet returns.','No positive profitability inference is made from connectivity or activity.']};}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){const r=study(process.argv[2],process.argv[4]);fs.writeFileSync(process.argv[3],JSON.stringify(r,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({observedMarkets:r.observedMarkets,eventCount:r.sourceEventCount,topCandidates:r.candidates.filter(c=>c.eligibleForFurtherReconciliation).slice(0,8)},null,2));}
