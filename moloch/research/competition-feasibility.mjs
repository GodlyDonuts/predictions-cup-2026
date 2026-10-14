import fs from 'node:fs';import {createHash} from 'node:crypto';
import {ActivityStudy} from '../activity.mjs';import {amount,money} from '../../paper/fixed.mjs';
const output=process.argv[2];if(!output)throw Error('new output required');
const directory='runtime/moloch/live-sig-20261014-40/capture/stream',raw=fs.readFileSync(directory+'/events.jsonl','utf8'),status=JSON.parse(fs.readFileSync(directory+'/stats.json'));
const study=new ActivityStudy();for(const line of raw.trim().split(/\r?\n/))study.row(JSON.parse(line));
if(status.status!=='completed'||status.sequence!==study.sequence||status.previous!==study.previous)throw Error('TERMINAL_STREAM');
const leaderboard=JSON.parse(fs.readFileSync('runtime/moloch/leaderboard-20261014-1/receipts.jsonl','utf8').trim().split(/\r?\n/)[0]);
if(leaderboard.status!==200||leaderboard.data.period!=='all'||leaderboard.data.sort!=='pnl')throw Error('REFERENCE_LEADERBOARD');
const leader=leaderboard.data.leaderboard.find(r=>r.rank===1);if(!leader||!Number.isFinite(leader.pnl))throw Error('REFERENCE_PNL');
const observedAt=new Date().toISOString(),deadline='2026-11-04T17:00:00.000Z',remainingSeconds=(Date.parse(deadline)-Date.parse(observedAt))/1000;
if(remainingSeconds<=0)throw Error('DEADLINE_PASSED');
const referenceProfit=amount(leader.pnl.toFixed(6)),seconds=(Date.parse(study.lastAt)-Date.parse(study.firstAt))/1000;
let observedVolume=0,compatiblePairs=0;const exchanges=[];
for(const m of study.markets.values()){
 const groups=new Map();for(const t of m.flow){observedVolume+=t.quantity;const g=groups.get(t.exchangeId)??{bid:0,ask:0};if(t.priorObservableSide==='YES')g.bid+=t.quantity;if(t.priorObservableSide==='NO')g.ask+=t.quantity;groups.set(t.exchangeId,g);}
 for(const [exchangeId,g]of groups){const pairs=Math.min(g.bid,g.ask);compatiblePairs+=pairs;if(pairs)exchanges.push({exchangeId,...g,compatiblePairs:pairs});}
}
const scenarios=[];
for(const participationBps of [100,1000,10000])for(const edgeTicks of [1,2,3]){
 // Count compatible shares within each exchange; never hedge unrelated markets.
 const packages=exchanges.reduce((n,e)=>n+BigInt(e.compatiblePairs)*BigInt(participationBps)/10000n,0n),profit=packages*5000n*BigInt(edgeTicks),rate=Number(profit)/1000000/seconds;
 scenarios.push({participationBps,edgeTicks,assumedCompletedPackages:Number(packages),hypotheticalWindowProfit:money(profit),hypotheticalProfitPerDay:rate*86400,referenceRequiredRateMultiple:rate?Number(referenceProfit)/1000000/remainingSeconds/rate:null});
}
const result={schemaVersion:1,observedAt,deadlineUtc:deadline,rulesSource:'https://predictionscup.com/rules/',settlementSource:'https://sig.thesuper.market/docs/settlement-and-payouts',referenceLeaderboardObservedAt:leaderboard.receivedAt,referenceReportedLeaderPnl:money(referenceProfit),referenceIsVerifiedFinalWinningThreshold:false,remainingDays:remainingSeconds/86400,referenceRequiredProfitPerDay:Number(referenceProfit)/1000000/remainingSeconds*86400,referenceRequiredDailyCompoundedReturnFrom100000:Math.pow(1+Number(referenceProfit)/1000000/100000,86400/remainingSeconds)-1,sourceSha256:createHash('sha256').update(raw).digest('hex'),sourceFinalHash:study.previous,sourceEvents:study.sequence,window:{from:study.firstAt,to:study.lastAt,seconds},observedVolume,compatiblePairs,exchanges:exchanges.sort((a,b)=>b.compatiblePairs-a.compatiblePairs),scenarios,contestOrders:0,profitabilityEstablished:false,limitations:['Older provisional leaderboard P&L is a reference, not current realized profit or the eventual winning balance.','Observed broadcasts are best-effort, incomplete and span a short window; activity is not a future-volume forecast.','Compatible bid/ask prints need not occur at matching prices or after the same quote activation.','Scenario assumes all attributed pairs complete at a fixed net spread; ignores queue obstruction, adverse selection, capital occupancy and market impact. Actual achievable profit can be negative.','The scenario is not a mathematical bound on future profits and does not authorize increasing live participation.']};
fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({referencePnl:result.referenceReportedLeaderPnl,remainingDays:result.remainingDays,requiredDailyProfit:result.referenceRequiredProfitPerDay,compatiblePairs,scenarios},null,2));
