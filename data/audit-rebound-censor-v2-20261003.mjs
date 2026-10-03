import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {studyDipMarkouts} from '../live-paper/dip-markout-study.mjs';
import {compareReboundPaths as legacy} from '../live-paper/rebound-path-study.mjs';
import {compareReboundPaths as fixed} from '../live-paper/rebound-path-study-v2.mjs';
import {canonicalJson,atomicJson} from '../execution/ledger.mjs';
const read=p=>JSON.parse(fs.readFileSync(p)),need=(ok,c)=>{if(!ok)throw Error(c);};
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),original=read('data/rebound-path-subsequent-20261003-v1.json');
const priceBytes=fs.readFileSync(path.join(rt,'market-prices.jsonl')).subarray(0,original.sources.price.bytes),bookBytes=fs.readFileSync(path.join(rt,'public-books.jsonl')).subarray(0,original.sources.book.bytes);
for(const[k,b]of Object.entries({price:priceBytes,book:bookBytes}))need(createHash('sha256').update(b).digest('hex')===original.sources[k].sha256,'ORIGINAL_CAPTURE_CHANGED');
const baseBank=read('data/dip-markout-protocol-20261003-v1.json'),universe=read('live-paper/price-universe.json');
const dips=studyDipMarkouts({priceBytes,bookBytes,universe,bank:baseBank}),parse=b=>b.toString().split('\n').slice(0,-1).map(JSON.parse);
const input={events:dips.events,prices:parse(priceBytes),books:parse(bookBytes),cutoff:original.cutoff};
const before=legacy(input),after=fixed(input);need(canonicalJson(before.cases)===canonicalJson(original.cases),'ORIGINAL_REPLAY_DIFFERS');
const observed=result=>{const out={};for(const c of result.cases)for(const[strategy,b]of Object.entries(c.branches))if(b)for(const[latency,t]of Object.entries(b.trials))for(const kind of ['quote','depth'])if(t[kind].stage==='entered')for(const[h,x]of Object.entries(t[kind].exits))if(x.status==='observed')out[[c.id,strategy,latency,kind,h].join(':')]={entryPrice:t[kind].entryPrice,entryAt:t[kind].entryAvailableAt,rawMarkout:x.rawMarkout,adverseMarkout:x.adverseMarkout,exitPrice:x.exitPrice,sourceHash:x.sourceHash};return out;};
need(canonicalJson(observed(before))===canonicalJson(observed(after)),'COMPLETED_ECONOMICS_CHANGED');
need(canonicalJson(before.cases.map(c=>c.id))===canonicalJson(after.cases.map(c=>c.id)),'OPPORTUNITY_DENOMINATORS_CHANGED');
const statuses=result=>Object.fromEntries(Object.entries(result.groups).map(([phase,g])=>[phase,Object.fromEntries(Object.entries(g.strategies).map(([name,s])=>[name,s.outcomes]))]));
const result={at:new Date().toISOString(),mode:'same_capture_terminal_censor_correction_audit',originalBankHash:original.bankHash,originalCutoff:original.cutoff,through:original.through,sources:original.sources,
 opportunities:before.cases.length,originalReplayMatched:true,completedEconomicPaths:Object.keys(observed(before)).length,completedEconomicsUnchanged:true,opportunityDenominatorsUnchanged:true,
 beforeStatuses:statuses(before),afterStatuses:statuses(after),networkRequests:0,paperFills:0,competitionOrders:0,profitabilityEstablished:false,
 limitation:'Retrospective correction audit on the original capture, not a new prospective strategy result. Original v1 artifacts remain unchanged.'};
atomicJson('data/rebound-censor-correction-audit-20261003.json',result);console.log(JSON.stringify({opportunities:result.opportunities,completedEconomicPaths:result.completedEconomicPaths,completedEconomicsUnchanged:true,originalReplayMatched:true}));
