import {decimalMicros,formatMicros,payloadHash,canonicalPayload} from './core.mjs';
import {effectiveFillPrice} from './side-accounting.mjs';
const scale=1000000n;
const need=(x,c)=>{if(!x)throw Error(c);};
const side=x=>x==='yes'||x==='no';
const time=x=>typeof x==='string'&&Number.isFinite(Date.parse(x));
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const money=x=>{need(typeof x==='string','EXACT_DECIMAL_REQUIRED');return decimalMicros(x);};
const signedMoney=x=>{need(typeof x==='string','EXACT_DECIMAL_REQUIRED');return x.startsWith('-')?-decimalMicros(x.slice(1)):decimalMicros(x);};
const signedFormat=x=>x<0n?'-'+formatMicros(-x):formatMicros(x);
const price=x=>{const n=money(x);need(n>=5000n&&n<=995000n&&n%5000n===0n,'INVALID_TICK');return n;};
const sign=x=>x==='yes'?1:-1;

// Pure attribution projection, not an execution permit or account reconciliation.
// Canonicalization uses the complete pre-order signed position, never a guessed
// holding or a dashboard mark. Unknown collateral economics remain rejected.
export function projectCanonicalFifo({wallet,binding,ack,rows,source,readAt}){
 need(wallet&&binding&&ack&&time(readAt)&&time(binding.createdAt)
  &&Date.parse(readAt)>=Date.parse(binding.createdAt),'INVALID_CANONICAL_INPUT');
 need(id(binding.orderId)&&id(binding.exchangeId)&&id(binding.marketId)
  &&typeof binding.tournamentId==='string'&&binding.tournamentId.length>0
  &&typeof binding.ownerId==='string'&&binding.ownerId.length>0
  &&wallet.ownerId===binding.ownerId&&wallet.exchangeId===binding.exchangeId
  &&wallet.marketId===binding.marketId&&wallet.tournamentId===binding.tournamentId,'ATTRIBUTION_SCOPE');
 need(side(binding.side)&&['buy','sell'].includes(binding.action)
  &&Number.isSafeInteger(binding.quantity)&&binding.quantity>0&&binding.quantity<=100,'INVALID_CANONICAL_TERMS');
 const applied=wallet.appliedOrderIds??[];
 need(Array.isArray(applied)&&applied.length<=10000&&applied.every(id)
  &&new Set(applied).size===applied.length&&!applied.includes(binding.orderId),'ORDER_ALREADY_PROJECTED_OR_INVALID_HISTORY');
 const limit=price(binding.limit),cash=money(wallet.cash),realized=signedMoney(wallet.realizedPnl);
 need(cash>=0n&&Array.isArray(wallet.lots)&&wallet.lots.length<=10000,'INVALID_FIFO_WALLET');
 const keys=new Set();let held=0,initialBasis=0n,heldSide=null;
 const lots=wallet.lots.map(l=>{
  need(typeof l.lotId==='string'&&l.lotId.length>0&&!keys.has(l.lotId)
   &&side(l.side)&&Number.isSafeInteger(l.quantity)&&l.quantity>0
   &&time(l.openedAt)&&Date.parse(l.openedAt)<=Date.parse(binding.createdAt),'INVALID_FIFO_LOT');
  keys.add(l.lotId);heldSide??=l.side;need(l.side===heldSide,'MIXED_SIGNED_LOTS');
  held+=l.quantity;need(Number.isSafeInteger(held),'FIFO_QUANTITY_OVERFLOW');
  const unit=price(l.entryPrice);initialBasis+=BigInt(l.quantity)*unit;return {...l,entryPrice:formatMicros(unit)};
 });
 for(let i=1;i<lots.length;i++)need(Date.parse(lots[i].openedAt)>=Date.parse(lots[i-1].openedAt),'FIFO_LOT_CLOCK');
 const signed=held*sign(heldSide||'yes');
 need(Number.isSafeInteger(wallet.accountSignedQuantity)&&wallet.accountSignedQuantity===signed,'UNATTRIBUTED_ACCOUNT_HOLDING');
 const backed=binding.action==='sell'&&heldSide===binding.side&&held>=binding.quantity;
 const canonicalAction=binding.action==='buy'||!backed?'buy':'sell';
 const complement=binding.action==='sell'&&!backed;
 const canonicalSide=complement?(binding.side==='yes'?'no':'yes'):binding.side;
 const canonicalLimit=complement?scale-limit:limit;
 need(String(ack.orderId)===binding.orderId&&ack.exchangeId===binding.exchangeId
  &&ack.side===canonicalSide&&ack.action===canonicalAction&&ack.quantity===binding.quantity
  &&price(ack.price)===canonicalLimit&&ack.all===null,'CANONICAL_ACK_OR_COLLATERAL_UNVERIFIED');
 need(Array.isArray(rows)&&rows.length<=10000&&typeof ack.open==='boolean'
  &&Number.isSafeInteger(ack.remainingQuantity)&&ack.remainingQuantity>=0
  &&Number.isSafeInteger(ack.quantityTraded)&&ack.quantityTraded>=0
  &&ack.quantityTraded+ack.remainingQuantity===binding.quantity,'INCOMPLETE_CANONICAL_ACK');
 const seen=new Map();
 for(const f of rows){
  need(Number.isSafeInteger(f.id)&&f.id>0&&String(f.orderId)===binding.orderId
   &&f.exchangeId===binding.exchangeId&&f.marketId===binding.marketId
   &&f.tournamentId===binding.tournamentId&&f.side===canonicalSide
   &&Number.isSafeInteger(f.quantity)&&f.quantity!==0&&Math.sign(f.quantity)===sign(canonicalSide)
   &&time(f.filledAt)&&Date.parse(f.filledAt)>=Date.parse(binding.createdAt)
   &&Date.parse(f.filledAt)<=Date.parse(readAt),'CANONICAL_FILL_BINDING');
  const unit=decimalMicros(effectiveFillPrice(f.price,canonicalSide,source));
  need(canonicalAction==='buy'?unit<=canonicalLimit:unit>=canonicalLimit,'CANONICAL_FILL_LIMIT');
  const row={id:f.id,quantity:Math.abs(f.quantity),unit:formatMicros(unit),filledAt:f.filledAt};
  if(seen.has(f.id))need(canonicalPayload(seen.get(f.id))===canonicalPayload(row),'CONFLICTING_CANONICAL_FILL');else seen.set(f.id,row);
 }
 const fills=[...seen.values()].sort((a,b)=>Date.parse(a.filledAt)-Date.parse(b.filledAt)||a.id-b.id);
 need(fills.reduce((q,f)=>q+f.quantity,0)===ack.quantityTraded
  &&(ack.open||ack.remainingQuantity===0||typeof ack.terminalReasonCode==='string'&&ack.terminalReasonCode.length>0),'CANONICAL_FILL_TOTAL');
 let cashDelta=0n,realizedDelta=0n,nominal=0n,closedQuantity=0,openedQuantity=0;
 for(const f of fills){
  let left=f.quantity;const unit=money(f.unit);nominal+=BigInt(left)*unit;
  if(canonicalAction==='sell'||lots.length&&lots[0].side!==canonicalSide){
   const proceeds=canonicalAction==='sell'?unit:scale-unit;
   while(left&&lots.length){const l=lots[0],q=Math.min(left,l.quantity),n=BigInt(q);cashDelta+=n*proceeds;realizedDelta+=n*(proceeds-money(l.entryPrice));closedQuantity+=q;left-=q;l.quantity-=q;if(!l.quantity)lots.shift();}
   need(canonicalAction!=='sell'||left===0,'SELL_EXCEEDS_ATTRIBUTED_LOTS');
  }
  if(left){const lotId='order:'+binding.orderId+':fill:'+f.id;need(!keys.has(lotId),'FIFO_LOT_ID_COLLISION');keys.add(lotId);lots.push({lotId,side:canonicalSide,quantity:left,entryPrice:f.unit,openedAt:f.filledAt});cashDelta-=BigInt(left)*unit;openedQuantity+=left;}
 }
 need(money(ack.totalCost)===nominal,'CANONICAL_ACK_PRINCIPAL_MISMATCH');
 need(cash+cashDelta>=0n,'FIFO_CASH_UNDERFLOW');
 const basis=lots.reduce((n,l)=>n+BigInt(l.quantity)*money(l.entryPrice),0n);
 need(cashDelta+basis-initialBasis===realizedDelta,'FIFO_ACCOUNTING_IDENTITY');
 const result={schemaVersion:1,bindingHash:payloadHash(binding),inputWalletHash:payloadHash(wallet),
  evidenceHash:payloadHash({ack,rows,source,readAt}),canonicalAction,canonicalSide,complement,
  filledQuantity:ack.quantityTraded,closedQuantity,openedQuantity,nominalPrincipal:formatMicros(nominal),
  cashDelta:signedFormat(cashDelta),realizedDelta:signedFormat(realizedDelta),remainingReservation:canonicalAction==='buy'&&ack.open?formatMicros(BigInt(ack.remainingQuantity)*canonicalLimit):'0.000000',
  wallet:{...wallet,cash:formatMicros(cash+cashDelta),realizedPnl:signedFormat(realized+realizedDelta),lots,
   accountSignedQuantity:lots.reduce((n,l)=>n+sign(l.side)*l.quantity,0),appliedOrderIds:[...applied,binding.orderId]},
  executionAuthority:false,authoritativeReconciliation:false};
 return {...result,projectionHash:payloadHash(result)};
}
