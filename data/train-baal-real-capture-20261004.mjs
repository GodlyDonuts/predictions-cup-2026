import fs from 'node:fs';
import path from 'node:path';
import {performance} from 'node:perf_hooks';
import {readTrainingSource} from '../live-paper/training-source.mjs';
import {trainPortfolioBank,trainPortfolioBankParallel} from '../live-paper/portfolio-bank.mjs';
import {validateConfig,digest} from '../live-paper/model.mjs';
import {DEFAULT_HORIZONS,parseTrainingHorizons} from '../live-paper/training-horizons.mjs';
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const args=process.argv.slice(2),options=new Map();
for(let i=0;i<args.length;i+=2){if(!['--cohort','--end-byte','--workers','--horizons'].includes(args[i])||options.has(args[i])||!args[i+1])throw Error('Invalid research arguments');options.set(args[i],args[i+1]);}
const name=options.get('--cohort')||'baal-net-20261004-a';
if(!/^baal-net-20261004-[a-z0-9-]{1,16}$/.test(name))throw Error('Invalid research cohort');
const endByte=options.has('--end-byte')?Number(options.get('--end-byte')):null;
if(endByte!==null&&(!Number.isSafeInteger(endByte)||endByte<1))throw Error('Invalid frozen prefix');
const workers=Number(options.get('--workers')||1);
if(!Number.isSafeInteger(workers)||workers<1||workers>8)throw Error('Invalid research worker count');
const horizons=options.has('--horizons')?parseTrainingHorizons(options.get('--horizons')):DEFAULT_HORIZONS;
const dir=path.join(runtime,'research-cohorts',name);
if(fs.existsSync(dir))throw Error('Research cohort exists; no overwrite');
const universe=JSON.parse(fs.readFileSync('live-paper/price-universe.json'));
const config=validateConfig({...JSON.parse(fs.readFileSync('live-paper/config.json')),
 strategyUniverse:'diversified-dip-v1',marketIds:[...new Set(universe.exchanges.filter(x=>x.option==='YES').map(x=>x.marketId))],
 initialCash:'100.000000',cashReserve:'90.000000',maxExposure:'10.000000',feeBps:0,
 latencyMs:3000,orderShares:1,maxPositionShares:1,depthFractionBps:100,slippageTicks:1});
fs.mkdirSync(dir,{recursive:true,mode:0o700});
const started=performance.now();let stage='verify_original_source',last=0;
fs.writeFileSync(path.join(dir,'config.json'),JSON.stringify(config,null,2),{flag:'wx',mode:0o600});
try{
 const source=readTrainingSource(path.join(runtime,'journal.jsonl'),{endByte,onProgress:x=>{
  if(x.bytes-last>=512*1024*1024){last=x.bytes;console.log(JSON.stringify({stage,verifiedBytes:x.bytes,records:x.records,endBytes:x.endBytes,rssMiB:Math.round(process.memoryUsage().rss/1048576)}));}
 }});
 const metadata={at:new Date().toISOString(),name,sourceBytes:source.sourceBytes,sourceRecords:source.sourceRecords,
  sourceHash:source.sourceHash,sourceThrough:source.finalEventClock,sourceFile:'journal.jsonl',
  verifyMs:performance.now()-started,markets:config.marketIds.length,configHash:digest(config),fees:0,
  sourcePreserved:true,competitionOrders:0,scope:'Historical screening; later untouched prospective data is still required.'};
 fs.writeFileSync(path.join(dir,'source.json'),JSON.stringify(metadata,null,2),{flag:'wx',mode:0o600});
 stage='fit_and_portfolio_selection';console.log(JSON.stringify({...metadata,stage,rssMiB:Math.round(process.memoryUsage().rss/1048576)}));
 const fitStarted=performance.now(),fitOptions={family:'ridge',horizons,onProgress:x=>console.log(JSON.stringify({...x,elapsedMs:Math.round(performance.now()-fitStarted),rssMiB:Math.round(process.memoryUsage().rss/1048576)}))};
 const bank=workers===1?trainPortfolioBank(source,config,fitOptions):await trainPortfolioBankParallel(source,config,{...fitOptions,workers});
 fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(bank,null,2),{flag:'wx',mode:0o600});
 const p=bank.portfolioSelection;
 const result={...metadata,at:new Date().toISOString(),status:'completed',bankHash:digest(bank),
  family:bank.family,horizons,workers,snapshots:bank.acceptedSnapshots,attempted:bank.candidatesAttempted,trained:bank.modelsTrained,
  modelMarginWallets:p.testedModelMarginPairs,decision:p.decision,selected:bank.candidatePlan.candidates,
  windows:p.windows,holdout:p.holdout,
  validationLeaders:p.validation.slice(0,5).map(x=>({id:x.id,eligible:x.eligible,rejectionReasons:x.rejectionReasons,blockPnl:x.blockPnl,last:x.points.at(-1)})),
  labelDiagnostics:bank.labelDiagnostics,
  fitAndReplayMs:performance.now()-fitStarted,totalMs:performance.now()-started,peakRssMiB:Math.round(process.resourceUsage().maxRSS/1024),
  productionActivation:false,prospectiveWalletStarted:false,profitabilityEstablished:false};
 fs.writeFileSync(path.join(dir,'result.json'),JSON.stringify(result,null,2),{flag:'wx',mode:0o600});
 fs.writeFileSync('data/'+name+'-result.json',JSON.stringify(result,null,2)+'\n',{flag:'wx'});
 fs.writeFileSync('data/baal-real-training-result-20261004.json',JSON.stringify(result,null,2)+'\n');
 console.log(JSON.stringify({status:result.status,decision:result.decision,trained:result.trained,modelMarginWallets:result.modelMarginWallets,holdout:result.holdout,elapsedMs:result.totalMs,peakRssMiB:result.peakRssMiB,competitionOrders:0}));
}catch(e){
 const safe=new Set(['Observation exceeds event clock','Missing account exclusions','Invalid request chronology','Book version rollback','Observation time moved backward','Conflicting equal book version','Pre-invalidation snapshot cannot recover']);
 const code=typeof e.message==='string'&&(safe.has(e.message)||/^(Source|Incomplete|No accepted|Insufficient|Need at least|Invalid|Chronological|Training source|Journal)/.test(e.message))?e.message:'RESEARCH_FAILURE';
 const frames=(e.stack||'').split('\n').slice(1,4).map(x=>x.match(/([^/\\]+\.mjs:\d+:\d+)/)?.[1]).filter(Boolean);
 const failed={at:new Date().toISOString(),name,status:'failed',stage,reason:code,errorType:e.name,frames,elapsedMs:performance.now()-started,sourcePreserved:true,competitionOrders:0};
 fs.writeFileSync(path.join(dir,'failure.json'),JSON.stringify(failed,null,2),{flag:'wx',mode:0o600});
 fs.writeFileSync('data/baal-real-training-failure-20261004.json',JSON.stringify(failed,null,2)+'\n');
 console.error(JSON.stringify(failed));process.exitCode=1;
}
