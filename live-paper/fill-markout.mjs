import {amount,money,TICK,instant} from '../paper/fixed.mjs';
const need=(v,c)=>{if(!v)throw Error('FILL_MARKOUT_'+c);};
const level=(book,side,quantity)=>book[side].find(l=>Math.floor(l.quantity/100)>=quantity);
// Observed quote quality only. It does not create a sale, assume queue fills,
// change wallet accounting or treat hypothetical proceeds as realized profit.
export function fillMarkout(fill,entryRow,laterRows,{horizonsMs=[5000,60000,300000],maximumLateMs=20000,throughAt}={}){
 need(fill.action==='buy'&&Number.isSafeInteger(fill.quantity)&&fill.quantity>0&&fill.fee==='0.000000','FILL_SCOPE');
 const entered=instant(fill.at),entry=entryRow.payload.book;
 need(entryRow.sequence===fill.sourceSequence&&entryRow.hash===fill.sourceHash&&entry.marketId===fill.marketId&&entry.exchangeId===fill.exchangeId&&entry.versionKey===fill.sourceVersion,'EXACT_ENTRY_SOURCE');
 need(entry.requestStartedAt===fill.requestStartedAt&&entry.receivedAt===fill.receivedAt&&instant(entryRow.at)<=entered&&instant(entry.receivedAt)<=entered,'ENTRY_CLOCK');
 const q=BigInt(fill.quantity),paid=amount(fill.price),ask=entry.asks.find(l=>Math.floor(l.quantity/100)>=fill.quantity&&amount(l.price)+TICK===paid),bid=level(entry,'bids',fill.quantity);need(ask&&bid,'ENTRY_DEPTH_OR_PRICE');
 const rawAsk=amount(ask.price),entryBid=amount(bid.price),through=instant(throughAt),entryCostComponents={observedSpread:money((entryBid-rawAsk)*q),modeledEntryTick:money((rawAsk-paid)*q),modeledExitTick:money(-TICK*q)};
 const horizons=horizonsMs.map(ms=>{need(Number.isSafeInteger(ms)&&ms>0,'HORIZON');const target=entered+ms;
  const row=laterRows.find(r=>r.kind==='book'&&r.payload.book.marketId===fill.marketId&&r.payload.book.exchangeId===fill.exchangeId&&instant(r.payload.book.requestStartedAt)>=target&&instant(r.at)<=through);
  if(!row)return{horizonMs:ms,status:through<target?'not_matured':'missing_later_book',realizedPnl:null};const b=row.payload.book,received=instant(b.receivedAt),requested=instant(b.requestStartedAt),sale=level(b,'bids',fill.quantity);
  if(received-target>maximumLateMs||received-requested>20000)return{horizonMs:ms,status:'capture_gap',receivedAt:b.receivedAt,realizedPnl:null};
  if(!sale||amount(sale.price)<TICK)return{horizonMs:ms,status:'insufficient_later_depth',realizedPnl:null};const futureBid=amount(sale.price),priceChange=(futureBid-entryBid)*q,net=(futureBid-TICK-paid)*q;
  need(amount(entryCostComponents.observedSpread)+amount(entryCostComponents.modeledEntryTick)+amount(entryCostComponents.modeledExitTick)+priceChange===net,'DECOMPOSITION');
  return{horizonMs:ms,status:'observed',requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,publicationSequence:row.sequence,publicationHash:row.hash,rawBid:money(futureBid),subsequentBidChange:money(priceChange),rawQuoteRoundtrip:money((futureBid-rawAsk)*q),modeledLiquidationNet:money(net),entryCostComponents,realizedPnl:null};
 });return{marketId:fill.marketId,exchangeId:fill.exchangeId,filledAt:fill.at,quantity:fill.quantity,paidPrice:fill.price,rawEntryAsk:ask.price,rawEntryBid:bid.price,entryCostComponents,horizons,fees:'0.000000',simulatedSales:0,profitabilityEstablished:false};
}
