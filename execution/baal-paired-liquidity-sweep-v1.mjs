import {createHash} from 'node:crypto';
import {payloadHash} from './core.mjs';
import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {normalizeOfficialFillTick} from './api-tick-price.mjs';
import {amount, price, money, instant, SCALE, TICK} from '../paper/fixed.mjs';

const need=(ok,code)=>{if(!ok)throw Error('PAIRED_SWEEP_'+code);};
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const sha=x=>createHash('sha256').update(x).digest('hex');
function decimalRational(text){
  need(typeof text==='string'&&text.length<=128,'EXACT_PRICE_SOURCE');
  const m=/^(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(text);need(m,'EXACT_PRICE_SOURCE');
  const exponent=Number(m[3]??0),fraction=m[2]??'';need(Number.isSafeInteger(exponent)&&Math.abs(exponent)<=128,'EXACT_PRICE_SOURCE');
  let numerator=BigInt(m[1]+fraction),scale=fraction.length-exponent;
  if(scale<0){numerator*=10n**BigInt(-scale);scale=0;}
  while(scale>0&&numerator%10n===0n){numerator/=10n;scale--;}
  return{numerator,scale};
}
const sameRational=(a,b)=>a.scale===b.scale&&a.numerator===b.numerator;

// This is price/depth mathematics, not an ownership, collateral or fill proof.
// The execution controller must independently verify the contract predicate,
// fresh account, funding, exact request and subsequent fills of BOTH legs.
function curve(leg,binding,at){
  need(id(leg.marketId)&&id(leg.exchangeId),'LEG_SCOPE');
  const r=leg.receipt;
  need(r?.method==='GET'&&r.status===200&&r.endpoint===`/markets/${leg.marketId}/orderbook?tournamentId=${binding.tournamentId}&depth=20`,'RECEIPT_SCOPE');
  need(typeof r.bodyText==='string'&&hex(r.sourceBodyHash)&&sha(r.bodyText)===r.sourceBodyHash,'BODY_HASH');
  const start=instant(r.requestStartedAt),end=instant(r.receivedAt);
  need(start<=end&&end<=at&&at-start<=20000,'SOURCE_AGE');
  const p=parseNumberLexemes(r.bodyText),d=p.data;
  need(!Object.hasOwn(d,'marketId')||d.marketId===leg.marketId,'MARKET');
  need(d.contexts?.length===1&&d.contexts[0].type==='tournament'&&d.contexts[0].tournament?.id===binding.tournamentId,'TOURNAMENT');
  const rows=d.contexts[0].orderbook?.exchanges;
  need(rows?.length===1&&rows[0].exchangeId===leg.exchangeId&&rows[0].option==='YES'&&Array.isArray(rows[0].bids)&&rows[0].bids.length<=20,'BINARY_BOOK');
  let previous=SCALE,total=0;
  const levels=rows[0].bids.map((l,i)=>{
    const normalized=normalizeOfficialFillTick(l.price),yes=price(normalized.price),q=l.quantity;
    const priceLexeme=p.numbers.get(JSON.stringify(['contexts',0,'orderbook','exchanges',0,'bids',i,'price']));
    const rational=decimalRational(priceLexeme),exactPrice=sameRational(rational,decimalRational(normalized.price));
    // Permit exact official decimals and the enumerated actual IEEE spelling,
    // never arbitrary raw precision that Number happens to round onto a tick.
    need(exactPrice||sameRational(rational,decimalRational(String(l.price))),'EXACT_PRICE_SOURCE');
    const lex=p.numbers.get(JSON.stringify(['contexts',0,'orderbook','exchanges',0,'bids',i,'quantity']));
    need(yes>=TICK&&yes<SCALE&&yes<previous,'SORTED_TICKS');previous=yes;
    need(Number.isSafeInteger(q)&&q>0&&typeof lex==='string'&&/^[1-9]\d*(\.0+)?$/.test(lex)&&BigInt(lex.split('.')[0])===BigInt(q),'WHOLE_DEPTH');
    total+=q;need(Number.isSafeInteger(total),'DEPTH_BOUND');
    return {noAsk:SCALE-yes,quantity:q};
  });
  return {levels,total,start,end,sourceHash:r.sourceBodyHash};
}

function consume(c,q){
  let remaining=q,cost=0n,limit=null;
  for(const l of c.levels){const n=Math.min(l.quantity,remaining);cost+=l.noAsk*BigInt(n);remaining-=n;if(n)limit=l.noAsk;if(!remaining)break;}
  return remaining?null:{cost,limit};
}

export function planBaalPairedLiquiditySweep({binding,legs,availableNominalCapital,
  maximumLegCost='25.000000',maximumEventCost='200.000000',maximumQuantity=100,
  minimumMarginPerMatchedShare='0.010000',now}){
  need(binding?.ownerId==='Codex'&&typeof binding.runId==='string'&&typeof binding.tournamentId==='string'&&hex(binding.reviewHash)&&hex(binding.eventId),'BINDING');
  need(Array.isArray(legs)&&legs.length===2&&new Set(legs.map(l=>l.marketId)).size===2&&new Set(legs.map(l=>l.exchangeId)).size===2,'DISTINCT_PAIR');
  need(Number.isSafeInteger(maximumQuantity)&&maximumQuantity>0&&maximumQuantity<=100,'QUANTITY_CAP');
  const at=instant(now),cash=amount(availableNominalCapital),legCap=amount(maximumLegCost),eventCap=amount(maximumEventCost),margin=price(minimumMarginPerMatchedShare);
  need(cash>=0n&&cash<=1000n*SCALE&&legCap>0n&&legCap<=25n*SCALE&&eventCap>0n&&eventCap<=200n*SCALE&&margin>=TICK,'NOMINAL_CAPS');
  const curves=legs.map(l=>curve(l,binding,at));
  need(Math.max(...curves.map(c=>c.end))-Math.min(...curves.map(c=>c.start))<=10000,'SOURCE_SKEW');
  const maximum=Math.min(maximumQuantity,...curves.map(c=>c.total)),candidates=[];
  for(let q=1;q<=maximum;q++){
    const consumed=curves.map(c=>consume(c,q));
    const maxima=consumed.map(x=>x.limit*BigInt(q)),total=maxima[0]+maxima[1];
    if(maxima.some(x=>x>legCap)||total>cash||total>eventCap)continue;
    const unitMargin=SCALE-consumed[0].limit-consumed[1].limit;
    if(unitMargin<margin)continue;
    candidates.push({quantity:q,limits:consumed.map(x=>money(x.limit)),maximumLegCosts:maxima.map(money),maximumNominalCost:money(total),
      displayedDepthNotional:money(consumed.reduce((n,x)=>n+x.cost,0n)),conditionalOrdinaryMarginLowerBound:money(unitMargin*BigInt(q)),
      conditionalOrdinaryPayout:money(SCALE*BigInt(q)),unitMarginLowerBound:money(unitMargin)});
  }
  candidates.sort((a,b)=>{const x=amount(a.conditionalOrdinaryMarginLowerBound),y=amount(b.conditionalOrdinaryMarginLowerBound);if(x!==y)return x>y?-1:1;
    const ac=amount(a.maximumNominalCost),bc=amount(b.maximumNominalCost);return ac===bc?b.quantity-a.quantity:ac<bc?-1:1;});
  // Exact historical control: one percent of each individual BEST rung only.
  const controlCap=Math.min(maximumQuantity,...curves.map(c=>c.levels.length?Math.floor(c.levels[0].quantity/100):0));
  const control=candidates.filter(c=>c.quantity<=controlCap&&curves.every((x,i)=>price(c.limits[i])===x.levels[0].noAsk))[0]??null;
  const result={schemaVersion:1,mode:'paired_price_bounded_depth_sweep_v1',evaluatedAt:now,binding:{...binding},
    selected:candidates[0]??null,onePercentBestRungControl:control,candidates,marketIds:legs.map(l=>l.marketId),exchangeIds:legs.map(l=>l.exchangeId),
    sourceHashes:curves.map(c=>c.sourceHash),sourceStartedAt:legs.map(l=>l.receipt.requestStartedAt),
    conditionalFloorAssumption:'Two distinct mutually exclusive party-winner YES predicates for the same race; ordinary paired NO payout at least one per matched share.',
    unvalidatedSemanticReview:true,partialOrUnfilledLegsInvalidatePackageMargin:true,administrativeOutcomesNotGuaranteed:true,
    collateralSavingsAssumed:'0.000000',fees:'0.000000',executionAuthority:false,orderEligible:false,profitabilityEstablished:false,actualFills:0};
  return {...result,proposalHash:payloadHash(result)};
}

// A read/experiment ranking only. Actual matching and funding remain serialized.
export function rankDisjointBaalSweeps({plans,availableNominalCapital,maximumNewMarkets=20}){
  need(Array.isArray(plans)&&Number.isSafeInteger(maximumNewMarkets)&&maximumNewMarkets>=0&&maximumNewMarkets<=20,'RANK_INPUT');
  let remaining=amount(availableNominalCapital);need(remaining>=0n&&remaining<=1000n*SCALE,'RANK_CAPITAL');
  const usedMarkets=new Set(),usedExchanges=new Set(),usedEvents=new Set(),selected=[];
  const ordered=plans.filter(p=>p?.selected).map(p=>{const{proposalHash,...body}=p;need(payloadHash(body)===proposalHash&&p.executionAuthority===false&&p.orderEligible===false,'RANK_SOURCE');return p;});
  ordered.sort((a,b)=>{const x=amount(a.selected.conditionalOrdinaryMarginLowerBound),y=amount(b.selected.conditionalOrdinaryMarginLowerBound);return x===y?a.proposalHash.localeCompare(b.proposalHash):x>y?-1:1;});
  for(const p of ordered){if(p.marketIds.some(m=>usedMarkets.has(m))||p.exchangeIds.some(ex=>usedExchanges.has(ex))||usedEvents.has(p.binding.eventId)||usedMarkets.size+2>maximumNewMarkets)continue;
    const choice=p.candidates.find(c=>amount(c.maximumNominalCost)<=remaining);if(!choice)continue;
    remaining-=amount(choice.maximumNominalCost);for(const m of p.marketIds)usedMarkets.add(m);for(const ex of p.exchangeIds)usedExchanges.add(ex);usedEvents.add(p.binding.eventId);selected.push({proposalHash:p.proposalHash,eventId:p.binding.eventId,marketIds:p.marketIds,exchangeIds:p.exchangeIds,choice});
  }
  return {selected,remainingNominalCapital:money(remaining),mode:'greedy_disjoint_dollar_margin_read_ranking',globallyOptimal:false,executionAuthority:false,orderEligible:false};
}
