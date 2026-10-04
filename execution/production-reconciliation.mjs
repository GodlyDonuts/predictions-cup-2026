import {payloadHash,conservativeCashFloor,formatMicros} from './core.mjs';
import {accountFills} from '../live-paper/account.mjs';
const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',base='/tournaments/midterm-elections/portfolio/fills?limit=100';
const need=(x,c)=>{if(!x)throw Error('PRODUCTION_RECONCILIATION_'+c);};
export function fillsReadAuthorized(spec){return spec?.method==='GET'&&spec.body===undefined&&spec.bodyJson===undefined&&/^\/tournaments\/midterm-elections\/portfolio\/fills\?limit=100(?:&cursor=[A-Za-z0-9%._~-]+)?$/.test(spec.endpoint);}
export async function reconcileProductionAccount({transport,dispatch,universe,now=Date.now,maxPages=20}){
 const history=await transport.orders('all'),fills=[],pages=[],seen=new Set(),cursors=new Set();let cursor,sequence,firstRowsHash,firstHasMore;
 for(let page=0;page<maxPages;page++){
  const spec={method:'GET',endpoint:base+(cursor?'&cursor='+encodeURIComponent(cursor):'')};need(fillsReadAuthorized(spec),'FILL_ROUTE');
  const r=await dispatch(spec),p=accountFills(r),coverage=r.data.coverage,pg=r.data.pagination;
  need(r.status>=200&&r.status<300&&coverage.complete===true,'FILL_COVERAGE');
  need(pg.limit===100&&Number.isSafeInteger(coverage.projectedThroughSequence)&&coverage.projectedThroughSequence>=0,'FILL_PAGINATION');
  if(page===0){firstRowsHash=payloadHash(p.rows);firstHasMore=p.hasMore;}else need(coverage.projectedThroughSequence>=sequence,'FILL_PROJECTION_REGRESSED');
  sequence=coverage.projectedThroughSequence;
  for(const f of p.rows){need(!seen.has(f.id)&&universe.exchanges.some(x=>x.exchangeId===f.exchangeId&&x.marketId===f.marketId),'FILL_IDENTITY');seen.add(f.id);fills.push(f);}
  pages.push({receivedAt:r.receivedAt,bodyHash:payloadHash(r.data),coverage});
  if(!pg.hasMore){need(pg.nextCursor===null,'FILL_PAGINATION');break;}
  need(p.rows.length>0&&typeof pg.nextCursor==='string'&&pg.nextCursor.length>0&&!cursors.has(pg.nextCursor),'FILL_PAGINATION');
  cursors.add(pg.nextCursor);cursor=pg.nextCursor;need(page+1<maxPages,'PAGE_LIMIT');
 }
 // The projection watermark can advance from other market/account activity.
 // Verify this account's immutable newest fills directly after the full scan.
 const head=await dispatch({method:'GET',endpoint:base}),headView=accountFills(head);
 need(head.status>=200&&head.status<300&&payloadHash(headView.rows)===firstRowsHash&&headView.hasMore===firstHasMore&&head.data.coverage.projectedThroughSequence>=sequence,'ACCOUNT_FILLS_CHANGED');
 pages.push({receivedAt:head.receivedAt,bodyHash:payloadHash(head.data),coverage:head.data.coverage,kind:'newest_fills_recheck'});
 // Refresh current exposure after the historical scans, then cash last. Old
 // history is not misrepresented as an atomic account snapshot.
 const positions=await transport.positions(),orders=await transport.orders('open'),cash=await transport.tournament(),at=new Date(now()).toISOString();
 need(cash.data.status==='active'&&cash.data.isPendingEnrolment===false,'TOURNAMENT');
 const times=[positions.receivedAt,orders.receivedAt,cash.receivedAt,pages[0]?.receivedAt].sort();
 need(times.every(t=>Number.isFinite(Date.parse(t))&&Date.parse(t)<=Date.parse(at))&&Date.parse(at)-Date.parse(times[0])<=90000,'FRESHNESS');
 const receipt={schemaVersion:1,tournamentId:TID,complete:true,at,oldestReceivedAt:times[0],newestReceivedAt:times.at(-1),positions,orders,cash,history,fills:{complete:true,rows:fills,pages},cashFloor:formatMicros(conservativeCashFloor(cash.data.myBalance)),limitations:['Sequential complete reads, not an atomic exchange snapshot. Each dispatch reconciles current cash/positions/orders again.','Existing orders and inventory retain their original attribution.']};
 return{receipt,receiptHash:payloadHash(receipt),summary:{complete:true,tournamentId:TID,oldestReceivedAt:receipt.oldestReceivedAt,newestReceivedAt:receipt.newestReceivedAt,receiptHash:payloadHash(receipt),positions:positions.data.positions.filter(x=>x.quantity!==0).length,openOrders:orders.data.data.length,historicalOrders:history.data.data.length,fills:fills.length}};
}
