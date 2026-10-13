import fs from 'node:fs';import readline from 'node:readline';import path from 'node:path';import {pathToFileURL} from 'node:url';
import {instantNs} from '../feed/contract.mjs';
const check=(ok,code)=>{if(!ok)throw Error('SIG_SOURCE_'+code);};
function decimal(v){const n=typeof v==='string'?Number(v):v;check(Number.isFinite(n)&&n>0&&n<1&&Math.abs(n*200-Math.round(n*200))<1e-7,'PRICE_TICK');return(Math.round(n*200)/200).toFixed(6);}
export function receiptEvents(r){
  check(r.source==='official_sig_api_v1'&&r.status===200&&instantNs(r.requestStartedAt)<=instantNs(r.receivedAt),'RECEIPT');
  const scope={tournamentId:r.tournamentId,marketId:r.marketId,exchangeId:r.exchangeId};
  const u=new URL(r.endpoint,'https://sig.thesuper.market/api/v1');check(u.searchParams.get('tournamentId')===r.tournamentId,'TOURNAMENT');
  if(r.kind==='book'){
    check(u.pathname===`/markets/${r.marketId}/orderbook`,'BOOK_ENDPOINT');
    const context=r.data.contexts?.find(c=>c.type==='tournament'&&c.tournament?.id===r.tournamentId);check(context,'EXPLICIT_BOOK_CONTEXT');
    const b=context.orderbook.exchanges.find(b=>String(b.exchangeId)===r.exchangeId);check(b&&Number.isSafeInteger(b.asOf?.sequence),'ENGINE_BOOK');
    if(!b.bids?.length||!b.asks?.length)return[{type:'gap',receivedAt:r.receivedAt,exchangeId:r.exchangeId}];
    const levels=rows=>rows.map(l=>({price:decimal(l.price),quantity:l.quantity}));
    return[{type:'book',receivedAt:r.receivedAt,book:{...scope,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt,sourceAt:b.asOf.at,sequence:String(b.asOf.sequence),bids:levels(b.bids),asks:levels(b.asks)}}];
  }
  if(r.kind==='trades'){
    check(u.pathname===`/exchanges/${r.exchangeId}/trades`&&r.data.exchangeId===r.exchangeId&&r.data.marketId===r.marketId,'TAPE_SCOPE');
    const events=[...r.data.data].sort((a,b)=>instantNs(a.createdAt)<instantNs(b.createdAt)?-1:instantNs(a.createdAt)>instantNs(b.createdAt)?1:a.id.localeCompare(b.id)).filter(t=>t.price!==null&&Number(t.price)>0&&Number(t.price)<1).map(t=>({type:'trade',...scope,id:t.id,executedAt:t.createdAt,receivedAt:r.receivedAt,yesPrice:decimal(t.price),quantity:t.size,idNamespace:'sig_reporting_tape_only',sourceRequestStartedAt:r.requestStartedAt}));
    // Only a complete single-page, original requested window grants coverage.
    // A cursor page by itself never authenticates the missing prefix.
    if(!u.searchParams.has('cursor')&&r.data.pagination?.hasMore===false&&r.data.pagination?.nextCursor===null&&r.data.coverage?.complete===true&&Number.isSafeInteger(r.data.coverage.projectedThroughSequence)){
      check(instantNs(r.data.from)===instantNs(u.searchParams.get('from'))&&instantNs(r.data.to)===instantNs(u.searchParams.get('to')),'COVERAGE_WINDOW');
      events.push({type:'coverage',...scope,receivedAt:r.receivedAt,from:r.data.from,to:r.data.to,complete:true,projectedThroughSequence:String(r.data.coverage.projectedThroughSequence)});
    }return events;
  }
  check(r.kind==='history','KIND');return[];
}
export async function convert(file,output){
  const fd=fs.openSync(output,'wx');let count=0,last=null;
  try{for await(const line of readline.createInterface({input:fs.createReadStream(file),crlfDelay:Infinity})){if(!line.trim())continue;const r=JSON.parse(line.replace(/^\uFEFF/,''));check(last===null||instantNs(r.receivedAt)>=last,'RECEIPT_ORDER');last=instantNs(r.receivedAt);for(const event of receiptEvents(r)){fs.writeSync(fd,JSON.stringify(event)+'\n');count++;}}}finally{fs.closeSync(fd);}
  return{events:count,networkRequests:0,contestOrders:0,reason:'Only complete single-page requested windows carry coverage; paginated prefixes remain unresolved.'};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){convert(process.argv[2],process.argv[3]).then(console.log).catch(e=>{console.error(e.message);process.exitCode=1;});}
