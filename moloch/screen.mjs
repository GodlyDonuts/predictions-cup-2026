import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {receiptEvents} from './sig-source.mjs';
const check=(ok,code)=>{if(!ok)throw Error('SCREEN_'+code);};
const read=file=>fs.readFileSync(file,'utf8').replace(/^\uFEFF/,'');
const receipts=file=>read(file).split(/\r?\n/).filter(x=>x.trim()).map(JSON.parse);
const sha=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
export function selectPairs(rows,count=8){
  check(Number.isInteger(count)&&count>=2&&count<=29,'SAMPLE_COUNT');
  const markets=new Map();let expected=null,cursor=null;
  for(const r of rows){
    check(r.source==='official_sig_api_v1'&&r.status===200&&r.kind==='catalog','CATALOG_SOURCE');
    const u=new URL(r.endpoint,'https://sig.thesuper.market');
    check(u.pathname==='/tournaments/midterm-elections/markets'&&u.searchParams.get('status')==='open'&&u.searchParams.get('cursor')===cursor,'CATALOG_CHAIN');
    check(expected===null||expected===r.data.pagination.total,'CATALOG_CHANGED');expected=r.data.pagination.total;
    for(const m of r.data.data){check(!markets.has(m.id)&&m.status==='open','CATALOG_DUPLICATE_OR_STATUS');markets.set(m.id,m);}
    cursor=r.data.pagination.nextCursor;
    check(r.data.pagination.hasMore===Boolean(cursor),'CATALOG_CURSOR');
  }
  check(cursor===null&&markets.size===expected,'CATALOG_INCOMPLETE');
  const universe=[...markets.values()].flatMap(m=>m.exchanges.map(e=>({marketId:String(m.id),exchangeId:String(e.id),title:m.title}))).sort((a,b)=>Number(a.marketId)-Number(b.marketId)||Number(a.exchangeId)-Number(b.exchangeId));
  check(universe.length>=count,'UNIVERSE_SMALL');
  // Fixed index-stratified exploration; metadata last prices are not liquidity signals.
  const chosen=new Map();for(let i=0;i<count;i++){const p=universe[Math.round(i*(universe.length-1)/(count-1))];chosen.set(p.exchangeId,p);}
  const control=universe.find(p=>p.marketId==='322'&&p.exchangeId==='1011');if(control)chosen.set(control.exchangeId,control);
  return{universeMarkets:markets.size,universeExchanges:universe.length,selection:'fixed equally spaced exchange-index sample plus prior market control; no price/volume alpha',pairs:[...chosen.values()]};
}
export function screenRows(rows){
  const pairs=new Map();
  for(const r of rows){
    const events=receiptEvents(r),key=r.exchangeId;
    let p=pairs.get(key);if(!p){p={marketId:r.marketId,exchangeId:key,book:null,tape:null};pairs.set(key,p);}
    check(p.marketId===r.marketId,'SCOPE_CONFLICT');
    if(r.kind==='book'){p.book=events.find(e=>e.type==='book')?.book??null;p.bookReceivedAt=r.receivedAt;}
    if(r.kind==='trades'){
      const unique=new Map();for(const e of events.filter(e=>e.type==='trade')){const prior=unique.get(e.id);check(!prior||prior.yesPrice===e.yesPrice&&prior.quantity===e.quantity,'TRADE_CONFLICT');unique.set(e.id,e);}
      p.tape=[...unique.values()];p.tapeReceivedAt=r.receivedAt;p.windowFrom=r.data.from;p.windowTo=r.data.to;p.complete=events.some(e=>e.type==='coverage');
    }
  }
  return [...pairs.values()].map(p=>{
    if(!p.book||!p.tape)return{marketId:p.marketId,exchangeId:p.exchangeId,eligible:false,reason:'missing two-sided book or tape'};
    const bid=p.book.bids[0],ask=p.book.asks[0],spreadTicks=Math.round((Number(ask.price)-Number(bid.price))*200);
    const volume=p.tape.reduce((sum,t)=>sum+Number(t.quantity),0);
    const bidThroughVolume=p.tape.filter(t=>Number(t.yesPrice)<Number(bid.price)).reduce((s,t)=>s+Number(t.quantity),0);
    const askThroughVolume=p.tape.filter(t=>Number(t.yesPrice)>Number(ask.price)).reduce((s,t)=>s+Number(t.quantity),0);
    const seconds=(Date.parse(p.windowTo)-Date.parse(p.windowFrom))/1000;
    check(seconds>0&&Number.isSafeInteger(volume)&&Number(bid.quantity)>0&&Number(ask.quantity)>0,'VOLUME_OR_DEPTH');
    const turnover=volume/Math.min(Number(bid.quantity),Number(ask.quantity));
    return{marketId:p.marketId,exchangeId:p.exchangeId,eligible:p.complete&&spreadTicks>0,trades:p.tape.length,volume,spreadTicks,bid:bid.price,ask:ask.price,bidQueue:Number(bid.quantity),askQueue:Number(ask.quantity),queueTurnover:turnover,
      bidThroughVolume,askThroughVolume,completeTapeWindow:p.complete,windowFrom:p.windowFrom,windowTo:p.windowTo,bookReceivedAt:p.bookReceivedAt,tapeReceivedAt:p.tapeReceivedAt,
      explorationScore:p.complete&&spreadTicks>0?spreadTicks*Math.log1p(turnover)*Math.log1p(p.tape.length):null};
  }).sort((a,b)=>(b.explorationScore??-1)-(a.explorationScore??-1));
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
  const [mode,input,output]=process.argv.slice(2);check(['select','report'].includes(mode),'MODE');
  const result=mode==='select'?selectPairs(receipts(input)): {sourceSha256:sha(input),candidates:screenRows(receipts(input)),contestOrders:0,profitabilityEstablished:false,
    limitations:['Exploration ranking only; sequential snapshots and recent tape are not synchronized execution evidence.','Through-volume compares past prints with current quotes, not quotes resting at execution time.','Queue turnover ignores aggressor direction; it is not a fill probability or expected return.','Incomplete tape is excluded from ranking; selection is a sample, not an exhaustive universe scan.']};
  fs.writeFileSync(output,JSON.stringify(mode==='select'?result.pairs:result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(mode==='select'?result:result.candidates));
}
