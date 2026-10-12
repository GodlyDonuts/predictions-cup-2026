import {amount,money,price,SCALE,TICK,instant} from '../paper/fixed.mjs';
import {assertSharedReferenceWalletV6} from '../paper/shared-reference-wallet-v6.mjs';

export function sharedWalletEconomicsV6({state,bank,at}){
 assertSharedReferenceWalletV6(state,bank);const time=instant(at);if(time<instant(state.lastAt))throw Error('SHARED_ECONOMICS_FUTURE_STATE');
 const reserved=Object.values(state.pending).filter(p=>p.action==='buy').reduce((n,p)=>n+price(p.limit)*BigInt(p.quantity),0n);
 const rows=[];let liquidation=0n,cost=0n,complete=true,fresh=true;
 for(const [marketId,p]of Object.entries(state.positions)){cost+=amount(p.cost);const spec=bank.instruments.find(i=>i.marketId===marketId),book=state.books[marketId];const base={marketId,quantity:p.quantity,principal:p.cost,bookAt:book?.receivedAt??null};
  if(!book||book.marketId!==marketId||book.exchangeId!==spec.exchangeId||instant(book.receivedAt)>time||instant(book.requestStartedAt)<instant(p.lastBoughtAt??p.enteredAt)+bank.latencyMs){rows.push({...base,complete:false,fresh:false,reason:'NO_INDEPENDENT_POST_ENTRY_MARK',liquidationValue:null,unrealizedPnl:null});complete=false;fresh=false;continue;}
  let remaining=p.quantity,value=0n,previous=SCALE;const levels=spec.side==='NO'?book.asks:book.bids;
  let top=true;for(const l of levels){const raw=spec.side==='NO'?SCALE-price(l.price):price(l.price);if(!Number.isSafeInteger(l.quantity)||l.quantity<1||raw>=previous)throw Error('SHARED_ECONOMICS_DEPTH');previous=raw;const px=raw-TICK;if(px<TICK)break;const used=state.liquidityUsage[marketId+':'+spec.side+':sell:'+book.versionKey+':'+money(raw)]??0;top=false;const quantity=Math.min(remaining,Math.max(0,Math.floor(l.quantity/100)-used));value+=px*BigInt(quantity);remaining-=quantity;if(!remaining)break;}
  const isFresh=time-instant(book.requestStartedAt)<=bank.maxBookAgeMs;fresh&&=isFresh;
  if(remaining){complete=false;rows.push({...base,complete:false,fresh:isFresh,reason:'INSUFFICIENT_LIQUIDATION_DEPTH',liquidationValue:null,unrealizedPnl:null});continue;}
  liquidation+=value;rows.push({...base,complete:true,fresh:isFresh,reason:isFresh?'INDEPENDENT_DEPTH_MARK':'DATED_DEPTH_MARK',liquidationValue:money(value),unrealizedPnl:money(value-amount(p.cost))});
 }
 const equity=complete?amount(state.cash)+liquidation:null,pnl=equity===null?null:equity-amount(bank.initialCash);
 if(pnl!==null&&pnl!==amount(state.realized)+liquidation-cost)throw Error('SHARED_ECONOMICS_WEALTH_IDENTITY');
 return{at,bankHash:bank.bankHash,cash:state.cash,reservedBuyCash:money(reserved),availableCash:money(amount(state.cash)-reserved),ownedPrincipal:money(cost),realized:state.realized,rows,allPositionsMarked:complete,allMarksFresh:fresh,datedNetPnl:pnl===null?null:money(pnl),freshNetPnl:complete&&fresh?money(pnl):null,datedEquity:equity===null?null:money(equity),marksAreCounterfactualLiquidation:true,actualOrders:0,profitabilityEstablished:false};
}
