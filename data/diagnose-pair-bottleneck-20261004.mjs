import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {journalLines} from '../live-paper/journal-lines.mjs';import {advanceBaalPair} from '../live-paper/baal-pair-paper-v2.mjs';
import {probabilityAccountGate} from '../live-paper/probability-receipt.mjs';import {amount,money,SCALE,TICK} from '../paper/fixed.mjs';import {payloadHash,atomicJson} from '../execution/ledger.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error('PAIR_DIAG_'+c);};
const active=read(path.join(r,'baal-pair-active.json')),dir=path.join(r,'baal-pair-cohorts',active.cohort),bank=read(path.join(dir,'bank.json')),file=path.join(dir,'journal.jsonl'),endByte=fs.statSync(file).size;
need(payloadHash(bank)===active.bankHash,'BANK');const hash=createHash('sha256'),counts={},examples=[],reasons={};let sequence=0,previous='',state,firstAt,lastAt,best=null;
const inc=k=>counts[k]=(counts[k]||0)+1;
for(const line of journalLines(file,{endByte})){if(!line)continue;const row=JSON.parse(line),{hash:recordHash,...body}=row;need(body.sequence===++sequence&&body.previous===previous&&sha(JSON.stringify(body))===recordHash,'CHAIN');hash.update(line+'\n');previous=recordHash;
 const event=row.event;if(event.kind==='genesis'){state=row.state.paper;firstAt=state.lastAt;continue;}
 state=advanceBaalPair(state,event,bank);need(payloadHash(state)===payloadHash(row.state.paper),'EXACT_REPLAY');lastAt=event.availableAt;
 if(event.kind!=='book'){inc('boundary');continue;}inc('book');const at=Date.parse(event.availableAt),books=bank.review.legs.map(l=>state.books[l.exchangeId]),admission=event.admission?.reason||'absent';reasons[admission]=(reasons[admission]||0)+1;
 if(!books.every(Boolean)){inc('missing_cached_pair');continue;}
 const accountGate=books.map(b=>probabilityAccountGate(event.accountEligibility,b,event.availableAt));if(accountGate.some(Boolean)){inc('account_gate');continue;}
 if(books.some(b=>at-Date.parse(b.requestStartedAt)>bank.policy.maxAgeMs)){inc('stale_pair');continue;}
 const skew=Math.max(...books.map(b=>Date.parse(b.receivedAt)))-Math.min(...books.map(b=>Date.parse(b.requestStartedAt)));if(skew>bank.policy.maxSkewMs){inc('pair_window_skew');continue;}inc('fresh_pair');
 if(!event.admission?.positiveConditionalCapacity){inc('capacity_not_positive');continue;}inc('positive_capacity_fresh_pair');
 const levels=books.map(b=>b.bids.find(l=>Math.floor(l.quantity*bank.policy.depthFractionBps/10000)>=1));if(levels.some(l=>!l)){inc('no_whole_share_depth');continue;}
 const units=levels.map(l=>SCALE-amount(l.price)+TICK),sum=units.reduce((a,b)=>a+b,0n);best=best===null||sum<best?sum:best;
 if(sum>SCALE-amount(bank.policy.minimumPackageMargin)){inc('package_cost_gate');continue;}inc('eligible_package_cost');
 if(!state.anchor&&!state.pending&&!state.package)inc('eligible_but_no_anchor');else if(state.anchor)inc('anchor_wait');else if(state.pending)inc('pending');else inc('owned_package');
 if(examples.length<10)examples.push({at:event.availableAt,sourceSequence:event.sourceSequence,skewMs:skew,cost:money(sum),anchor:state.anchor?{at:state.anchor.at,versions:state.anchor.versions}:null,pendingStage:state.pending?.stage??null,versions:books.map(b=>b.versionKey)});
}
const report={at:new Date().toISOString(),source:{cohort:active.cohort,bankHash:active.bankHash,bytes:endByte,records:sequence,hash:hash.digest('hex'),tailHash:previous,firstAt,lastAt},exactStateReplay:true,counts,admissionReasons:reasons,bestEligibleOneShareCost:best===null?null:money(best),examples,ending:{fills:state.fills.length,pending:state.pending?.stage??null,anchor:state.anchor,counts:state.counts},networkRequests:0,competitionOrders:0,profitabilityEstablished:false};
atomicJson('data/pair-bottleneck-20261004.json',report);console.log(JSON.stringify(report));
