import {amount,money,price,SCALE,TICK,instant} from './fixed.mjs';import {payloadHash} from '../execution/core.mjs';
// Research adaptation of inventory reservation pricing. No arrival intensity
// or profitability is presumed; maker fills require a separate causal model.
export function inventoryAwareQuotesV1({book,inventory,cashAvailable,variancePerSecond,riskAversion=10,horizonSeconds=300,inventoryScale=100,maximumShares=100,maximumOrderCost='100.000000',now}){
 const need=(v,c)=>{if(!v)throw Error('INVENTORY_QUOTE_'+c);};const at=instant(now);need(book&&at-instant(book.requestStartedAt)<=20000&&instant(book.receivedAt)<=at&&instant(book.requestStartedAt)<=instant(book.receivedAt),'FRESH_BOOK');
 const b=book.bids?.[0],a=book.asks?.[0];need(b&&a&&Number.isSafeInteger(b.quantity)&&b.quantity>0&&Number.isSafeInteger(a.quantity)&&a.quantity>0,'DEPTH');const bid=price(b.price),ask=price(a.price);need(bid<ask,'UNCROSSED');
 need(['yes','no'].every(k=>Number.isSafeInteger(inventory?.[k])&&inventory[k]>=0)&&Number.isFinite(variancePerSecond)&&variancePerSecond>=0&&variancePerSecond<=.25&&Number.isFinite(riskAversion)&&riskAversion>=0&&riskAversion<=1000&&Number.isFinite(horizonSeconds)&&horizonSeconds>0&&horizonSeconds<=3600&&Number.isSafeInteger(inventoryScale)&&inventoryScale>0&&Number.isSafeInteger(maximumShares)&&maximumShares>0&&maximumShares<=100,'PARAMETERS');
 const cash=amount(cashAvailable),cap=amount(maximumOrderCost);need(cash>=0n&&cap>0n&&cap<=100000000n,'FUNDED_CAP');
 const midpoint=Number(bid+ask)/2e6,microprice=(Number(ask)*b.quantity+Number(bid)*a.quantity)/(b.quantity+a.quantity)/1e6,q=(inventory.yes-inventory.no)/inventoryScale,penalty=riskAversion*variancePerSecond*horizonSeconds,reservation=microprice-q*penalty,halfSpread=Math.max(.005,(Number(ask-bid)/2e6)+penalty/2);
 const buyTicks=Math.max(1,Math.min(199,Math.floor((reservation-halfSpread)/.005))),sellTicks=Math.max(1,Math.min(199,Math.ceil((reservation+halfSpread)/.005)));need(buyTicks<sellTicks,'NONCROSSED_QUOTING');
 const yesBuy=BigInt(buyTicks)*TICK,yesSell=BigInt(sellTicks)*TICK,noBuy=SCALE-yesSell,noSell=SCALE-yesBuy;
 const buyBudget=cash<cap?cash:cap,oneUnitPackage=yesBuy+noBuy,buys=[],sells=[];let reserved=0n;
 // Reserve distinct old shares for direct sale and opposite-side acquisition.
 // Otherwise both fills could consume the same lot and create a naked sale.
 const sellYes=inventory.yes?Math.min(maximumShares,Math.max(1,Math.floor(inventory.yes/2))):0,sellNo=inventory.no?Math.min(maximumShares,Math.max(1,Math.floor(inventory.no/2))):0;
 if(sellYes)sells.push({side:'YES',action:'sell',quantity:sellYes,limit:money(yesSell)});if(sellNo)sells.push({side:'NO',action:'sell',quantity:sellNo,limit:money(noSell)});
 const addBuy=(side,limit,wanted)=>{if(wanted<1)return;const quantity=Math.min(maximumShares,wanted,Number((buyBudget-reserved)/limit));if(quantity>0){reserved+=limit*BigInt(quantity);buys.push({side,action:'buy',quantity,limit:money(limit)});}};
 if(yesBuy<ask&&noBuy<SCALE-bid){
  if(!inventory.yes&&!inventory.no){const units=Math.min(maximumShares,Number(buyBudget/oneUnitPackage));addBuy('YES',yesBuy,units);addBuy('NO',noBuy,units);}
  else{addBuy('YES',yesBuy,inventory.no?inventory.no-sellNo:Math.max(0,maximumShares-inventory.yes));addBuy('NO',noBuy,inventory.yes?inventory.yes-sellYes:Math.max(0,maximumShares-inventory.no));}
 }
 const out={schemaVersion:1,mode:'bounded_inventory_reservation_quote_research_v1',marketId:book.marketId,exchangeId:book.exchangeId,at:now,midpoint,microprice,inventoryUnits:q,reservationPrice:reservation,variancePerSecond,riskAversion,horizonSeconds,halfSpread,buys,sells,maximumQuoteReservation:money(reserved),conditionalComplementSurplusPerPairedUnit:money(SCALE-oneUnitPackage),fees:'0.000000',executionAuthority:false,networkRequests:0,estimatedFillProbability:null,profitabilityEstablished:false,assumptions:['Microprice is a heuristic rather than a calibrated future-value forecast.','Arrival rates and queue position are not estimated.','Inventory-driven ask can realize a loss; no profitability floor is fabricated.','Only later independently observed fills may affect a paper wallet.']};return{...out,proposalHash:payloadHash(out)};
}
