import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';
import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication-v2.mjs';
import {advanceNoMaker,assertNoMaker} from '../live-paper/no-maker-paper-v2.mjs';
import {amount,money,TICK,SCALE} from '../paper/fixed.mjs';
import {tapeTick} from '../live-paper/canonical-entry-exit-study.mjs';

const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const need=(ok,c)=>{if(!ok)throw Error('BAAL_ROUNDTRIP_'+c);};
const sha=b=>createHash('sha256').update(b).digest('hex');
const read=f=>JSON.parse(fs.readFileSync(f));
const complete=f=>{const b=fs.readFileSync(f);return b.subarray(0,b.lastIndexOf(10)+1);};
const rows=b=>b.length?b.toString().trimEnd().split('\n').map(JSON.parse):[];
const active=read(path.join(r,'no-maker-active.json'));
need(active.cohort==='no-maker-tape-20261004-v3','COHORT');
const dir=path.join(r,'no-maker-cohorts',active.cohort),bank=read(path.join(dir,'bank.json'));
need(payloadHash(bank)===active.bankHash,'BANK');
const bytes=complete(path.join(dir,'journal.jsonl')),journal=rows(bytes);
let previous='',paper=null;
for(const [i,row]of journal.entries()){
 const {hash,...body}=row;
 need(row.sequence===i+1&&row.previous===previous&&sha(JSON.stringify(body))===hash,'JOURNAL_CHAIN');previous=hash;
 if(i===0){need(row.event.kind==='genesis','GENESIS');paper=row.state.paper;assertNoMaker(paper,bank);continue;}
 const e=row.event;
 if(e.kind==='trade_page')paper=advanceNoMaker(paper,{kind:'trades',availableAt:e.availableAt,page:e.event},bank);
 else if(e.kind==='book'||e.kind==='paired_book')for(const b of e.books)paper=advanceNoMaker(paper,{kind:'book',availableAt:e.availableAt,book:b,accountEligibility:e.accountEligibility,entryAllowed:e.entryAllowed},bank);
 else if(e.kind==='boundary')paper=advanceNoMaker(paper,{kind:'boundary',availableAt:e.availableAt},bank);
 else need(e.kind==='other_trade_page','EVENT');
 need(payloadHash(paper)===payloadHash(row.state.paper),'EXACT_STATE_REPLAY');
}
const tapeBytes=complete(path.join(r,'anonymous-trades.jsonl')),pages=rows(tapeBytes),trades=new Map();previous='';
for(const [i,row]of pages.entries()){
 const{hash,...body}=row;need(row.sequence===i+1&&row.previous===previous&&sha(JSON.stringify(body))===hash,'TAPE_CHAIN');previous=hash;
 for(const t of row.event.trades)trades.set(t.key,{t,page:row.event,sequence:row.sequence,hash});
}
const bookBytes=complete(path.join(r,'public-books.jsonl'));
let books=createBookPublicationState(read('live-paper/price-universe.json'));
const wanted=new Set(journal.filter(x=>x.event.sourceSequence).map(x=>x.event.sourceSequence));
const bindings=new Map();
for(const row of rows(bookBytes)){
 books=applyBookPublication(books,row);
 if(wanted.has(books.sequence))bindings.set(books.sequence,books);
}
for(const row of journal.filter(x=>x.event.sourceSequence)){
 const b=bindings.get(row.event.sourceSequence);need(b?.hash===row.event.sourceHash,'PUBLICATION_BINDING');
 for(const source of row.event.books)need(payloadHash(b.books[source.instrument])===payloadHash(source),'BOOK_BINDING');
}
const results=[];
for(const [family,w]of Object.entries(paper.wallets)){
 for(let i=0;i<w.fills.length;i++){
  const buy=w.fills[i];if(buy.action!=='buy')continue;
  const sell=w.fills.slice(i+1).find(f=>f.exchangeId===buy.exchangeId&&f.action==='sell');if(!sell)continue;
  const t=trades.get(buy.source.key);need(t&&t.t.id===buy.source.id&&t.t.createdAt===buy.source.createdAt&&t.page.receivedAt===buy.source.firstAvailableAt&&tapeTick(t.t)?.price===buy.source.normalizedYesPrice,'ENTRY_SOURCE');
  const closed=journal.find(x=>x.state.paper.wallets[family].fills.some(f=>payloadHash(f)===payloadHash(sell)));
  need(closed&&closed.event.kind==='paired_book'||closed&&closed.event.kind==='book','EXIT_EVENT');
  const b=closed.event.books.find(b=>b.exchangeId===sell.exchangeId);
  need(b&&b.publicationHash===sell.source.publicationHash&&b.publicationSequence===sell.source.publicationSequence&&b.versionKey===sell.source.version,'EXIT_SOURCE');
  const prior=journal[closed.sequence-2].state.paper.wallets[family],risk=prior.risk[sell.exchangeId];
  need(risk&&Date.parse(b.requestStartedAt)>=Date.parse(risk.createdAt)+bank.policy.latencyMs,'INDEPENDENT_EXIT_REQUEST');
  need(Math.floor(b.asks[0].quantity*bank.policy.depthFractionBps/10000)>=sell.quantity&&amount(sell.price)===SCALE-amount(b.asks[0].price)-TICK,'EXIT_PRICE_DEPTH');
  results.push({family,marketId:buy.marketId,exchangeId:buy.exchangeId,quantity:buy.quantity,
   entryPrice:buy.price,entrySourceAt:buy.source.createdAt,entryKnownAt:buy.source.firstAvailableAt,entryBookedAt:buy.at,
   entryTradeId:buy.source.id,entryTapeSequence:t.sequence,entryTapeHash:t.hash,normalizedTradeYesPrice:buy.source.normalizedYesPrice,
   exitPrice:sell.price,exitBookedAt:sell.at,exitTrigger:risk.reason,exitTriggerAt:risk.createdAt,
   exitRequestStartedAt:b.requestStartedAt,exitReceiptAt:b.receivedAt,exitPublicationSequence:b.publicationSequence,exitPublicationHash:b.publicationHash,
   rawNoLiquidationBid:money(SCALE-amount(b.asks[0].price)),adverseExitTick:money(TICK),eligibleExitDepth:Math.floor(b.asks[0].quantity/100),
   realizedPnl:money((amount(sell.price)-amount(buy.price))*BigInt(buy.quantity)),fees:'0.000000',
   heldFromSourceMs:Date.parse(sell.at)-Date.parse(buy.source.createdAt),actualCompetitionFill:false});
 }
}
need(results.length===1&&results[0].realizedPnl==='-0.015000','FIRST_CLOSE');
const out={schemaVersion:1,at:new Date().toISOString(),cohort:active.cohort,bankHash:active.bankHash,exactStateReplay:true,
 sources:{makerJournal:{bytes:bytes.length,sha256:sha(bytes),sequence:journal.length,tailHash:journal.at(-1).hash},tradeTape:{bytes:tapeBytes.length,sha256:sha(tapeBytes),sequence:pages.length},publicBooks:{bytes:bookBytes.length,sha256:sha(bookBytes),sequence:books.sequence}},
 roundtrips:results,wallets:Object.fromEntries(Object.entries(paper.wallets).map(([k,w])=>[k,{cash:w.cash,realizedPnl:w.realized,fills:w.fills.length,positions:Object.keys(w.positions).length}])),
 accountingContinuation:bank.accountingContinuation.cohort,independentRoundtrips:1,profitabilityEstablished:false,competitionOrders:0,
 finding:'The paired capacity admission produced a standalone maker position. No partner fill completed a package. The holding-limit sale realized spread/adverse-tick cost; zero fees do not remove it.',
 limitations:['Strict price-through is a counterfactual fill, not proof of aggressor side or queue priority.','This first close includes the inherited V2 purchase and must not be added to predecessor accounting.','One correlated observation cannot establish profitability or select a strategy.']};
fs.writeFileSync('data/baal-maker-first-roundtrip-20261004.json',JSON.stringify(out,null,2)+'\n');
console.log(JSON.stringify({roundtrips:results.length,realizedPnl:results[0].realizedPnl,exitTrigger:results[0].exitTrigger,exactStateReplay:true,sourceBindingsVerified:true}));
