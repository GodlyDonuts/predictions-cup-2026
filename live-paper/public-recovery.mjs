import {createHash} from 'node:crypto';
import {amount,money,price,instant,sequence,TICK,SCALE} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';
import {observeRecovery,recoveryConfirmation,RECOVERY_CONFIRMATION_POLICY} from './public-recovery-confirmation.mjs';
export const PUBLIC_CONFIRMATION_POLICY=Object.freeze({...RECOVERY_CONFIRMATION_POLICY,windowMs:600000,maximumGapMs:180000});
export const PUBLIC_RECOVERY_VARIANTS=Object.freeze(['baseline','confirmed','pricecap'].flatMap(entry=>[
 {name:'legacy',takeProfit:'0.010000',stopLoss:'0.030000'},
 {name:'balanced',takeProfit:'0.020000',stopLoss:'0.020000'},
 {name:'reward',takeProfit:'0.030000',stopLoss:'0.020000'}
].map(payoff=>({id:entry+'_'+payoff.name,entry,payoff}))));

export const publicRecoveryHash=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
export const PUBLIC_RECOVERY_POLICY=Object.freeze({feeBps:0,historyMs:21600000,minimumMinutes:8,minimumSpanMs:1800000,
 maximumHistoryAgeMs:2700000,minimumDip:.02,minimumRelativeDip:.03,maximumDip:.1,minimumZ:1.5,
 maximumSpread:.02,minimumPrice:.05,maximumPrice:.95,entryLifetimeMs:120000,maximumQuoteAgeMs:20000,
 takeProfit:'0.010000',stopLoss:'0.030000',maximumHoldMs:3600000,cooldownMs:900000,
 initialCash:'100.000000',maxMarketCost:'25.000000',maxExposure:'100.000000',cumulativeBuyBudget:'100.000000',
 maxOpenMarkets:3,orderShares:10,depthFractionBps:100,slippageTicks:1,latenciesMs:[1000,3000]});
const need=(ok,m)=>{if(!ok)throw Error(m);},iso=x=>new Date(x).toISOString(),inc=(o,k)=>{o[k]=(o[k]||0)+1;};
const median=x=>{const a=[...x].sort((a,b)=>a-b),n=a.length;return n%2?a[(n-1)/2]:(a[n/2-1]+a[n/2])/2;};
function checkBank(b){need(b?.schemaVersion===1&&b.mode==='public_rest_recovery_paper'&&/^[a-f0-9]{64}$/.test(b.sourceScopeHash),'PUBLIC_RECOVERY_BANK');
 instant(b.cutoff);need(b.tournamentId==='bda92870-621e-47b0-bc3c-3602c5c26f55','PUBLIC_RECOVERY_TOURNAMENT');need(Number.isSafeInteger(b.startEventId)&&b.startEventId>=0,'PUBLIC_RECOVERY_START_EVENT');need(publicRecoveryHash(b.policy)===publicRecoveryHash(PUBLIC_RECOVERY_POLICY),'PUBLIC_RECOVERY_POLICY_CHANGED');
 need(publicRecoveryHash(b.variants)===publicRecoveryHash(PUBLIC_RECOVERY_VARIANTS),'PUBLIC_RECOVERY_VARIANTS_CHANGED');
 need(publicRecoveryHash(b.confirmationPolicy)===publicRecoveryHash(PUBLIC_CONFIRMATION_POLICY),'PUBLIC_RECOVERY_CONFIRMATION_CHANGED');
 need(Array.isArray(b.markets)&&b.markets.length>0&&b.markets.length<=256&&new Set(b.markets.map(x=>x.marketId)).size===b.markets.length&&new Set(b.markets.map(x=>x.exchangeId)).size===b.markets.length,'PUBLIC_RECOVERY_SCOPE');
 for(const m of b.markets)need(/^[1-9]\d*$/.test(m.marketId)&&/^[1-9]\d*$/.test(m.exchangeId),'PUBLIC_RECOVERY_ID');}
export function createPublicRecovery(bank){checkBank(bank);return {schemaVersion:1,bankHash:publicRecoveryHash(bank),lastEventId:bank.startEventId,lastAvailableAt:null,
 histories:{},recoveryPoints:{},seen:{},latest:{},invalid:{},warmBooks:0,observations:0,
 wallets:Object.fromEntries(bank.variants.flatMap(variant=>bank.policy.latenciesMs.map(latencyMs=>[variant.id+'_'+latencyMs,{variant:structuredClone(variant),latencyMs,cash:bank.policy.initialCash,
 realizedPnl:'0.000000',cumulativeBuys:'0.000000',positions:{},pending:{},outcomes:[],fills:[],roundTrips:[],gateCounts:{},signals:{},cooldowns:{},closedTrades:0,nextOrder:1}])) )};}
function checkedBook(e,bank){const b=e.book,m=bank.markets.find(x=>x.marketId===e.marketId);
 need(m&&e.tournamentId===bank.tournamentId&&e.exchangeId===m.exchangeId&&b?.marketId===m.marketId&&b.exchangeId===m.exchangeId&&b.instrument===m.exchangeId+':YES','PUBLIC_RECOVERY_BOOK_SCOPE');
 sequence(b.sequence);instantNs(b.sourceAt);need(typeof b.versionKey==='string'&&b.versionKey===b.sequence+':'+b.sourceAt,'PUBLIC_RECOVERY_VERSION');
 for(const side of ['bids','asks']){need(Array.isArray(b[side])&&b[side].length>0,'PUBLIC_RECOVERY_DEPTH');let previous=null;
  for(const l of b[side]){const p=price(l.price);need(Number.isSafeInteger(l.quantity)&&l.quantity>0,'PUBLIC_RECOVERY_QUANTITY');
   if(previous!==null)need(side==='bids'?p<previous:p>previous,'PUBLIC_RECOVERY_DEPTH_ORDER');previous=p;}}
 need(amount(b.bids[0].price)<amount(b.asks[0].price),'PUBLIC_RECOVERY_CROSSED');
 need(e.origin==='rest_observation','PUBLIC_RECOVERY_ORIGIN');
 need(typeof e.requestStartedAt==='string','PUBLIC_RECOVERY_REQUEST_CLOCK');
 const received=instant(e.quoteReceivedAt),confirmed=instant(e.confirmedAt),available=instant(e.availableAt);
 need(received<=confirmed+1000&&confirmed<=available,'PUBLIC_RECOVERY_CLOCK');if(e.requestStartedAt)need(instantNs(e.requestStartedAt)<=instantNs(e.quoteReceivedAt),'PUBLIC_RECOVERY_REQUEST_CLOCK');
 if(e.nextExpiryAt)instantNs(e.nextExpiryAt);return b;}
function appendHistory(s,e,p){const b=e.book,id=e.marketId,minute=Math.floor(instant(e.quoteReceivedAt)/60000),a=s.histories[id]||[];
 // Keep the first accepted version in each minute; broadcast intensity has no weight.
 if(!a.some(x=>x.minute===minute))a.push({minute,receivedAt:e.quoteReceivedAt,confirmedAt:e.confirmedAt,
  sourceAt:b.sourceAt,mid:(Number(amount(b.bids[0].price))+Number(amount(b.asks[0].price)))/2e6,versionKey:b.versionKey});
 a.sort((x,y)=>x.minute-y.minute);const newest=a.at(-1).minute*60000;
 s.histories[id]=a.filter(x=>x.minute*60000>=newest-p.historyMs).slice(-361);}
export function publicRecoveryReference(points,at,p=PUBLIC_RECOVERY_POLICY){const now=instant(at),a=(points||[]).filter(x=>instant(x.receivedAt)<now&&instant(x.confirmedAt)<=now&&instantNs(x.sourceAt)<BigInt(now)*1000000n&&x.minute*60000>=now-p.historyMs);
 if(a.length<p.minimumMinutes)return {ready:false,reason:'insufficient_minutes',samples:a.length};
 const first=instant(a[0].receivedAt),last=instant(a.at(-1).receivedAt);
 if(last-first<p.minimumSpanMs)return {ready:false,reason:'insufficient_history_span',samples:a.length};
 if(now-last>p.maximumHistoryAgeMs)return {ready:false,reason:'historical_reference_stale',samples:a.length};
 const center=median(a.map(x=>x.mid)),mad=median(a.map(x=>Math.abs(x.mid-center)));
 return {ready:true,source:'minute_balanced_prior_verified_book_midpoints',samples:a.length,center,robustDeviation:Math.max(.0025,mad*1.4826)};}
export function warmPublicRecovery(input,books,bank){checkBank(bank);need(input.bankHash===publicRecoveryHash(bank)&&input.lastEventId===bank.startEventId&&!input.lastAvailableAt,'PUBLIC_RECOVERY_WARM_AFTER_START');const s=structuredClone(input);
 for(const e of books){checkedBook(e,bank);need(instant(e.confirmedAt)<=instant(bank.cutoff)&&instant(e.quoteReceivedAt)<=instant(bank.cutoff),'PUBLIC_RECOVERY_FUTURE_WARM');appendHistory(s,e,bank.policy);s.recoveryPoints[e.marketId]=observeRecovery(s.recoveryPoints[e.marketId]||[],e,bank.confirmationPolicy);s.warmBooks++;}
 return s;}
function resolve(w,id,at,status){const order=w.pending[id];if(!order)return;const o=w.outcomes.find(x=>x.id===order.id);o.status=status;o.resolvedAt=at;o.unfilledQuantity=order.remaining;
 if(status==='expired')o.censoring=o.independentReceipts===0?'no_independent_in_window_quote':'observed_without_full_execution';delete w.pending[id];}
function assertWallet(w,p){let cost=0n;for(const pos of Object.values(w.positions)){need(Number.isSafeInteger(pos.quantity)&&pos.quantity>0&&amount(pos.cost)>0n,'PUBLIC_RECOVERY_POSITION');cost+=amount(pos.cost);}
 need(amount(w.cash)>=0n&&amount(w.cash)+cost-amount(p.initialCash)===amount(w.realizedPnl),'PUBLIC_RECOVERY_ACCOUNTING');
 need(cost<=amount(p.maxExposure)&&amount(w.cumulativeBuys)<=amount(p.cumulativeBuyBudget),'PUBLIC_RECOVERY_BUDGET');}
function fillPending(w,e,p){const id=e.marketId,o=w.pending[id];if(!o)return;const b=e.book,receipt=instant(e.quoteReceivedAt),confirmed=instant(e.confirmedAt);
 const independent=sequence(b.sequence)>sequence(o.sequence)&&instantNs(b.sourceAt)>instantNs(o.sourceAt);
 if(!independent||receipt<instant(o.at)+w.latencyMs||receipt>=instant(o.expiresAt)||confirmed>=instant(o.expiresAt)
  ||!e.continuity?.ready||!e.continuity?.continuous||e.nextExpiryAt&&instantNs(e.nextExpiryAt)<=instantNs(e.confirmedAt))return;
 const outcome=w.outcomes.find(x=>x.id===o.id);outcome.independentReceipts++;
 const levels=o.side==='buy'?b.asks:b.bids,limit=amount(o.limit);let remaining=o.remaining;
 for(const l of levels){const raw=amount(l.price);if(o.side==='buy'?raw>limit:raw<limit)break;
  const depth=Math.floor(l.quantity*p.depthFractionBps/10000),qty=Math.min(remaining,depth);if(!qty)continue;
  // This is a counterfactual executable opportunity, not queue priority or a real fill.
  const execution=o.side==='buy'?(raw+TICK<limit?raw+TICK:limit):(raw-TICK>limit?raw-TICK:limit),notional=execution*BigInt(qty);let gain=0n;
  if(o.side==='buy'){need(amount(w.cash)>=notional,'PUBLIC_RECOVERY_CASH');const pos=w.positions[id]||{quantity:0,cost:'0.000000',openedAt:e.confirmedAt,realizedPnl:'0.000000',enteredQuantity:0,entryCost:'0.000000'};
   pos.enteredQuantity+=qty;pos.entryCost=money(amount(pos.entryCost)+notional);
   pos.quantity+=qty;pos.cost=money(amount(pos.cost)+notional);w.positions[id]=pos;w.cash=money(amount(w.cash)-notional);w.cumulativeBuys=money(amount(w.cumulativeBuys)+notional);
  }else{const pos=w.positions[id];need(pos&&pos.quantity>=qty,'PUBLIC_RECOVERY_OVERSELL');const released=qty===pos.quantity?amount(pos.cost):amount(pos.cost)*BigInt(qty)/BigInt(pos.quantity);
   gain=notional-released;pos.quantity-=qty;pos.cost=money(amount(pos.cost)-released);pos.realizedPnl=money(amount(pos.realizedPnl)+gain);w.cash=money(amount(w.cash)+notional);w.realizedPnl=money(amount(w.realizedPnl)+gain);
   if(!pos.quantity){w.roundTrips.push({marketId:id,openedAt:pos.openedAt,closedAt:e.confirmedAt,quantity:pos.enteredQuantity,entryCost:pos.entryCost,realizedPnl:pos.realizedPnl});delete w.positions[id];w.closedTrades++;w.cooldowns[id]=iso(confirmed+p.cooldownMs);}}
  w.fills.push({orderId:o.id,marketId:id,side:o.side,quantity:qty,price:money(execution),realizedPnl:money(gain),fee:'0.000000',
   sourceVersion:b.versionKey,quoteReceivedAt:e.quoteReceivedAt,confirmedAt:e.confirmedAt,gatewayAvailableAt:e.availableAt,evidence:'counterfactual_executable_opportunity'});
  remaining-=qty;o.remaining=remaining;outcome.filledQuantity+=qty;if(!remaining)break;
 }
 // One accepted version consumes depth once, including partial fills.
 o.sequence=b.sequence;o.sourceAt=b.sourceAt;if(!remaining)resolve(w,id,e.confirmedAt,'filled');assertWallet(w,p);}
function fresh(s,e,now,p){return e.decisionEligible&&!s.invalid[e.marketId]&&e.continuity?.ready&&e.continuity?.continuous
 &&now-instant(e.quoteReceivedAt)>=0&&now-instant(e.quoteReceivedAt)<p.maximumQuoteAgeMs&&(!e.nextExpiryAt||instantNs(e.nextExpiryAt)>BigInt(now)*1000000n);}
function submit(w,e,at,side,quantity,limit,p,reason){const id=e.marketId,orderId='recovery-'+w.variant.id+'-'+w.latencyMs+'-'+w.nextOrder++;
 const o={id:orderId,side,quantity,remaining:quantity,limit,at,expiresAt:iso(instant(at)+p.entryLifetimeMs),sequence:e.book.sequence,sourceAt:e.book.sourceAt,reason};
 w.pending[id]=o;w.outcomes.push({...o,marketId:id,status:'pending',filledQuantity:0,independentReceipts:0});}
function decide(s,w,e,at,bank){const id=e.marketId,p={...bank.policy,...w.variant.payoff},b=e.book,now=instant(at);if(!fresh(s,e,now,p)){inc(w.gateCounts,'stale_or_invalid');return;}
 if(w.pending[id])return;const bid=amount(b.bids[0].price),ask=amount(b.asks[0].price),pos=w.positions[id];
 if(pos){const per=amount(pos.cost)/BigInt(pos.quantity),exit=bid-TICK;
  if(exit-per>=amount(p.takeProfit)||per-exit>=amount(p.stopLoss)||now-instant(pos.openedAt)>=p.maximumHoldMs)
   submit(w,e,at,'sell',pos.quantity,money(exit>0n?exit:TICK),p,'profit_loss_or_hold_exit');return;}
 const ref=publicRecoveryReference(s.histories[id],at,p);w.signals[id]={...ref,at,rawAsk:money(ask),rawBid:money(bid),
  paddedAsk:money(ask+TICK),paddedBid:money(bid-TICK),rawSpread:Number(ask-bid)/1e6,
  paddedSpread:Number(ask-bid+2n*TICK)/1e6,
  ...(ref.ready?{paddedDiscount:ref.center-Number(ask+TICK)/1e6,conditionalDiscount:ref.center-Number(ask)/1e6}: {})};
 if(!ref.ready){inc(w.gateCounts,ref.reason);return;}
 if(Date.parse(w.cooldowns[id]||'')>now){inc(w.gateCounts,'cooldown');return;}
 const occupied=new Set([...Object.keys(w.positions),...Object.keys(w.pending)]);if(occupied.size>=p.maxOpenMarkets){inc(w.gateCounts,'market_count');return;}
 const identity=bank.markets.find(x=>x.marketId===id),others=[...occupied].map(x=>bank.markets.find(m=>m.marketId===x));
 if(!identity.state||!identity.office){if(others.some(x=>!x.state||!x.office)){inc(w.gateCounts,'unknown_identity_concentration');return;}}
 if(identity.state&&others.filter(x=>x.state===identity.state).length>=2){inc(w.gateCounts,'state_concentration');return;}
 if(instant(e.quoteReceivedAt)<=instant(bank.cutoff)){inc(w.gateCounts,'pre_cutoff_quote');return;}
 const admitted=candidate=>{const a=Number(candidate)/1e6,bidValue=Number(bid-TICK)/1e6,d=ref.center-a;
  return a>=p.minimumPrice&&a<=p.maximumPrice&&a>bidValue&&a-bidValue<=p.maximumSpread+1e-12&&d>=p.minimumDip-1e-12&&d<=p.maximumDip
   &&d/Math.max(ref.center,.001)>=p.minimumRelativeDip&&d/ref.robustDeviation>=p.minimumZ;};
 const confirmation=recoveryConfirmation(s.recoveryPoints[id]||[],e,at,bank.confirmationPolicy);w.signals[id].recovery=confirmation;
 if(w.variant.entry!=='baseline'&&!confirmation.admitted){inc(w.gateCounts,confirmation.reason);return;}
 let limit;if(w.variant.entry==='pricecap'){if(ask>TICK&&admitted(ask))limit=ask-TICK;}else if(admitted(ask+TICK))limit=ask;else if(w.variant.entry==='baseline'&&ask>TICK&&admitted(ask))limit=ask-TICK;
 if(limit&&w.variant.entry!=='baseline'&&limit-(bid-TICK)>=amount(p.stopLoss)){inc(w.gateCounts,'insufficient_exit_headroom');return;}
 if(!limit){inc(w.gateCounts,'discount_spread_or_regime_gate');
  const a=Number(ask)/1e6,discount=ref.center-a;
  inc(w.gateCounts,a<p.minimumPrice||a>p.maximumPrice?'price_band':Number(ask-bid+TICK)/1e6>p.maximumSpread+1e-12?'spread':
   discount<p.minimumDip-1e-12?'minimum_discount':discount>p.maximumDip?'regime_change':
   discount/Math.max(ref.center,.001)<p.minimumRelativeDip?'relative_discount':'robust_z');return;}
 const reserved=Object.values(w.pending).filter(x=>x.side==='buy').reduce((n,x)=>n+amount(x.limit)*BigInt(x.remaining),0n),budget=amount(p.cumulativeBuyBudget)-amount(w.cumulativeBuys)-reserved;
 let qty=Math.min(p.orderShares,Number(amount(p.maxMarketCost)/limit),Number(budget/limit),Number((amount(w.cash)-reserved)/limit));
 if(qty<1){inc(w.gateCounts,'budget');return;}submit(w,e,at,'buy',qty,money(limit),p,limit===ask?'native_dip':'conditional_one_tick_lower');inc(w.gateCounts,'admitted');}
export function advancePublicRecovery(input,events,bank,{availableAt,evidenceThrough=availableAt,allowDecisions=true}){checkBank(bank);need(input.bankHash===publicRecoveryHash(bank),'PUBLIC_RECOVERY_BANK_CHANGED');const now=instant(availableAt),watermark=instant(evidenceThrough);
 need(watermark<=now&&(!input.lastEvidenceThrough||watermark>=instant(input.lastEvidenceThrough))&&typeof allowDecisions==='boolean','PUBLIC_RECOVERY_EVIDENCE_CLOCK');
 need(now>=instant(bank.cutoff)&&(!input.lastAvailableAt||now>=instant(input.lastAvailableAt)),'PUBLIC_RECOVERY_HOST_CLOCK');const s=structuredClone(input),last=new Map();
 for(const e of events){need(e.eventId===s.lastEventId+1&&instant(e.availableAt)<=now&&instant(e.confirmedAt)<=now,'PUBLIC_RECOVERY_EVENT_ORDER');
  need(e.tournamentId===bank.tournamentId&&bank.markets.some(x=>x.marketId===e.marketId&&x.exchangeId===e.exchangeId),'PUBLIC_RECOVERY_SCOPE');s.lastEventId=e.eventId;
  if(e.kind==='invalidate'){s.invalid[e.marketId]=true;delete s.latest[e.marketId];last.delete(e.marketId);for(const w of Object.values(s.wallets))resolve(w,e.marketId,e.confirmedAt,'invalidated');continue;}
  need(e.kind==='book','PUBLIC_RECOVERY_EVENT_KIND');const b=checkedBook(e,bank),old=s.seen[e.marketId];
  const contentHash=publicRecoveryHash({bids:b.bids,asks:b.asks});
  if(old){need(sequence(b.sequence)>=sequence(old.sequence)&&instantNs(b.sourceAt)>=instantNs(old.sourceAt),'PUBLIC_RECOVERY_SOURCE_ROLLBACK');
   if(b.sequence===old.sequence)need(b.sourceAt===old.sourceAt&&contentHash===old.contentHash,'PUBLIC_RECOVERY_SAME_VERSION_CHANGED');}
  s.seen[e.marketId]={sequence:b.sequence,sourceAt:b.sourceAt,contentHash};delete s.invalid[e.marketId];s.latest[e.marketId]=e;
  if(!old||b.sequence!==old.sequence)s.observations++;
  for(const w of Object.values(s.wallets))fillPending(w,e,bank.policy);
  s.recoveryPoints[e.marketId]=observeRecovery(s.recoveryPoints[e.marketId]||[],e,bank.confirmationPolicy);last.set(e.marketId,e);
 }
 // Resolve the entire ordered evidence batch before expiring old orders or making
 // new decisions. A delayed consumer cannot erase an adverse pre-expiry fill.
 for(const w of Object.values(s.wallets)){for(const [id,o]of Object.entries(w.pending))if(watermark>=instant(o.expiresAt))resolve(w,id,availableAt,'expired');
  if(allowDecisions)for(const e of last.values())decide(s,w,e,availableAt,bank);assertWallet(w,bank.policy);}
 for(const e of last.values())appendHistory(s,e,bank.policy);s.lastAvailableAt=availableAt;s.lastEvidenceThrough=evidenceThrough;return s;}
export function publicRecoveryReport(s,bank,at){need(s.bankHash===publicRecoveryHash(bank),'PUBLIC_RECOVERY_REPORT_BANK');const now=instant(at);
 return {mode:bank.mode,bankHash:s.bankHash,sourceScopeHash:bank.sourceScopeHash,updatedAt:at,cutoff:bank.cutoff,lastEventId:s.lastEventId,
 variants:bank.variants,confirmationPolicy:bank.confirmationPolicy,
 configuredMarkets:bank.markets.length,historyCoverage:{marketsWithBookHistory:Object.keys(s.histories).length,marketsWithRecoveryContext:Object.keys(s.recoveryPoints).length,
  usableHistoricalReferences:bank.markets.filter(m=>streamDipReferenceForReport(s.histories[m.marketId],at,bank.policy)).length},
 warmBooks:s.warmBooks,observations:s.observations,networkRequests:0,competitionOrders:0,feeBps:0,profitabilityEstablished:false,
 wallets:Object.fromEntries(Object.entries(s.wallets).map(([name,w])=>{let liquidation=0n,complete=true;const positions=Object.entries(w.positions).map(([id,pos])=>{
  const e=s.latest[id],ready=e&&fresh(s,e,now,bank.policy);let mark=null;if(ready){let q=pos.quantity,value=0n;for(const l of e.book.bids){const n=Math.min(q,Math.floor(l.quantity*bank.policy.depthFractionBps/10000));value+=(amount(l.price)>TICK?amount(l.price)-TICK:0n)*BigInt(n);q-=n;if(!q)break;}if(!q)mark=money(value);}
  if(mark===null)complete=false;else liquidation+=amount(mark);return {marketId:id,...pos,liquidationValue:mark,markReady:mark!==null,
   quoteReceivedAt:e?.quoteReceivedAt??null,quoteInvalidated:!ready,observationAgeMs:e?now-instant(e.quoteReceivedAt):null};});
 const wins=w.roundTrips.filter(x=>amount(x.realizedPnl)>0n),losses=w.roundTrips.filter(x=>amount(x.realizedPnl)<0n),grossWins=wins.reduce((n,x)=>n+amount(x.realizedPnl),0n),grossLosses=losses.reduce((n,x)=>n-amount(x.realizedPnl),0n);
 return [name,{variant:w.variant,latencyMs:w.latencyMs,cash:w.cash,realizedPnl:w.realizedPnl,cumulativeBuys:w.cumulativeBuys,closedTrades:w.closedTrades,positions,pending:Object.entries(w.pending).map(([marketId,o])=>({marketId,...o})),
  closedRoundTrips:{count:w.roundTrips.length,wins:wins.length,losses:losses.length,flat:w.roundTrips.length-wins.length-losses.length,winRate:w.roundTrips.length?wins.length/w.roundTrips.length:null,grossWins:money(grossWins),grossLosses:money(grossLosses),profitFactor:grossLosses?Number(grossWins)/Number(grossLosses):null,recent:w.roundTrips.slice(-30)},
  idealizedBreakEvenWinRate:Number(amount(w.variant.payoff.stopLoss))/Number(amount(w.variant.payoff.stopLoss)+amount(w.variant.payoff.takeProfit)),
  totalPnl:complete?money(amount(w.cash)+liquidation-amount(bank.policy.initialCash)):null,marksComplete:complete,
  clockExpiredAwaitingSource:Object.values(w.pending).filter(o=>now>=instant(o.expiresAt)).length,
  gateCounts:w.gateCounts,signals:w.signals,orders:w.outcomes.length,fills:w.fills.length,recentFills:w.fills.slice(-30),outcomes:w.outcomes.slice(-50)}];}))};}

function streamDipReferenceForReport(points,at,p){return publicRecoveryReference(points,at,p).ready;}
