import fs from 'node:fs';import path from 'node:path';import {payloadHash,atomicJson} from '../execution/ledger.mjs';import {marketActivityFocusV1} from '../research/market-activity-focus-v1.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),raw=fs.readFileSync(path.join(rt,'stats.json')),stats=JSON.parse(raw);
const universe=JSON.parse(fs.readFileSync('live-paper/price-universe.json')),now=new Date().toISOString();
if(stats.runtime?.status!=='running'||Date.now()-Date.parse(stats.reportedAt)>30000)throw Error('ACTIVITY_COLLECTOR_STALE');
const result=marketActivityFocusV1({markets:stats.runtime.tradeTape.markets,universe,now});
const report={...result,sourceStatsHash:payloadHash(stats),sourceReportedAt:stats.reportedAt,canonicalUniverseHash:payloadHash(universe),notAccountPortfolioData:true};
atomicJson('data/market-activity-focus-latest.json',report);
console.log(JSON.stringify({at:now,observedMarkets:result.rows.length,freshObserved:result.rows.filter(r=>r.status!=='observation_stale_or_missing').length,selected:result.selected.map(r=>({marketId:r.marketId,latestTradeAt:r.latestTradeAt})),quietMaine:result.rows.find(r=>r.marketId==='271')?.status,actualOrders:0}));
