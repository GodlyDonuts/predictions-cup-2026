import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';import {createHash} from 'node:crypto';
import {createRealtimeSpreadEconomics} from '../live-paper/realtime-spread-economics.mjs';
import {createFundedQuoteWallet} from '../live-paper/funded-quote-replay.mjs';
import {quoteOutcomeForecast} from '../live-paper/quote-outcome-forecast.mjs';
const root=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),modelFile='data/realtime-quote-selectors-20261005.json',raw=fs.readFileSync(modelFile),report=JSON.parse(raw);
const activeFile=JSON.parse(fs.readFileSync(path.join(root,'realtime-session.json'))).file;
const original=report.sources,additional=fs.readdirSync(root).filter(f=>f.startsWith('realtime-')&&f.endsWith('.jsonl')&&f>'realtime-2026-10-05T16-38'&&f<'realtime-2026-10-05T17-50'&&f!==activeFile).sort();
const sources=[...original,...additional.map(file=>({file,bytes:fs.statSync(path.join(root,file)).size}))];
const started=performance.now(),studies=[];
for(const bank of report.banks){if(bank.status!=='research_selector_frozen')continue;
 for(const [segment,startAt]of[['development_later_period',bank.testStartsAt],['subsequent_sealed_sessions','2026-10-05T16:38:18.000Z']])
 for(const selected of[false,true]){
  const wallet=createFundedQuoteWallet({policyId:bank.policy.id,startAt,selector:selected?q=>quoteOutcomeForecast(report,bank,q.x).selected:()=>true});
  const study=createRealtimeSpreadEconomics({policies:[bank.policy],decideQuote:wallet.decideQuote,onFill:wallet.onFill,onOutcome:wallet.onOutcome});
  studies.push({policy:bank.policy.id,segment,selector:selected?'frozen_arrival_and_conditional_value':'unfiltered_control',wallet,study});
 }
}
const manifestFile='data/funded-quote-replay-policy-20261005.json',out='data/funded-quote-replay-20261005.json';if(fs.existsSync(out))throw Error('FROZEN_REPLAY_EXISTS');
fs.writeFileSync(manifestFile,JSON.stringify({schemaVersion:1,frozenAt:new Date().toISOString(),modelSha256:createHash('sha256').update(raw).digest('hex'),sources:sources.map(({sha256,...s})=>({...s,sha256:sha256??null})),policies:report.banks.map(b=>b.policy),initialCash:'100.000000',maximumCommitted:'5.000000',maximumCumulativePurchases:'25.000000',maximumMarkets:3,fees:'0.000000',competitionOrders:0},null,2)+'\n',{flag:'wx'});
for(const source of sources){const h=createHash('sha256'),file=path.join(root,source.file),stream=fs.createReadStream(file);stream.on('data',b=>h.update(b));let rows=0;
 for await(const line of readline.createInterface({input:stream,crlfDelay:Infinity}))if(line){const row=JSON.parse(line);for(const s of studies)s.study.consume(row,{sourceId:source.file});rows++;}
 const sha256=h.digest('hex');if(fs.statSync(file).size!==source.bytes||source.sha256&&sha256!==source.sha256)throw Error('SOURCE_CHANGED');source.sha256=sha256;source.rows=rows;
 console.log(JSON.stringify({file:source.file,rows,elapsedMs:Math.round(performance.now()-started)}));
}
const result={schemaVersion:1,frozenAt:new Date().toISOString(),modelFile,modelSha256:createHash('sha256').update(raw).digest('hex'),sources,additionalSealedSessions:additional.length,elapsedMs:performance.now()-started,
 wallets:studies.map(s=>({policy:s.policy,segment:s.segment,selector:s.selector,wallet:s.wallet.report(),sourceMetrics:s.study.report().metrics})),
 networkRequests:0,competitionOrders:0,executionAuthority:false,profitabilityEstablished:false,
 limitations:['Development-period data were previously examined; subsequent sealed sessions are a new selector comparison, not prospective live competition performance.','Censored quote paths retain known holdings and unresolved reservations. Missing economic observations are not zero returns.','Arrival probabilities and conditional-value estimates are uncalibrated; no confidence or guaranteed profit follows from a point forecast.','Control and selected wallets are separate counterfactual alternatives and cannot be added as capital or profit.']};
fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
const summary={...result,wallets:result.wallets.map(w=>({...w,wallet:{...w.wallet,fills:w.wallet.fills.length,outcomes:w.wallet.outcomes.length}}))};
fs.writeFileSync('data/funded-quote-replay-summary-20261005.json',JSON.stringify(summary,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({elapsedMs:result.elapsedMs,additional:additional.length,wallets:summary.wallets.map(w=>({policy:w.policy,segment:w.segment,selector:w.selector,cash:w.wallet.cash,realized:w.wallet.realizedPnl,buys:w.wallet.buyFills,closed:w.wallet.closedTrades,reserved:w.wallet.reserved,basis:w.wallet.inventoryCost,complete:w.wallet.completeEconomicObservation,counts:w.wallet.counts}))},null,2));
