import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {journalLines} from './journal-lines.mjs';import {validateUniverse} from './breadth.mjs';import {validatePriceObservation} from './market-price-publication.mjs';
import {decimalMicros} from '../execution/core.mjs';
import {createRelativeHistory,observeRelativePrices,relativeDipFeatures,relativeIdentity,RELATIVE_DIP_POLICY} from './relative-dip.mjs';
const hash=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
export function verifiedRelativeInputs(files,universe,requiredPrefixes=[]){
 const snapshots=[],sources=[];
 for(const file of files){if(!fs.existsSync(file))continue;const size=fs.statSync(file).size;let sequence=0,previous='',bytes=0,lastAt=-Infinity;
  const required=requiredPrefixes.find(x=>x.file===path.basename(file));let prefixSeen=!required||required.sequence===0;
  for(const line of journalLines(file,{endByte:size})){const row=JSON.parse(line),{hash:digest,...body}=row,at=Date.parse(row.at);if(row.sequence!==sequence+1||row.previous!==previous||hash(body)!==digest||!Number.isFinite(at)||at<lastAt)throw Error('RELATIVE_SOURCE_CHAIN');sequence=row.sequence;previous=digest;lastAt=at;bytes+=Buffer.byteLength(line)+1;
   if(required&&sequence===required.sequence){if(digest!==required.hash)throw Error('RELATIVE_FROZEN_PREFIX_CHANGED');prefixSeen=true;}
   const snapshot=row.event?.bulkPrices??(row.event?.type==='market_price_observation'?row.event.snapshot:null);
   if(snapshot){validatePriceObservation(snapshot,universe);snapshots.push({snapshot,proof:{source:path.basename(file),sequence,hash:digest}});}
  }
  if(!prefixSeen)throw Error('RELATIVE_FROZEN_PREFIX_MISSING');sources.push({file:path.basename(file),sequence,hash:previous,bytes});
 }
 if(requiredPrefixes.some(p=>!sources.some(s=>s.file===p.file)))throw Error('RELATIVE_FROZEN_SOURCE_MISSING');
 snapshots.sort((a,b)=>Date.parse(a.snapshot.receivedAt)-Date.parse(b.snapshot.receivedAt));const seen=new Set();
 return {sources,snapshots:snapshots.filter(x=>{const key=hash(x.snapshot);if(seen.has(key))return false;seen.add(key);return true;})};
}
export function relativeEpisodeLabels(episodes,quotes,{horizonsMinutes=[5,15],maxDelayMs=60000,maxLatencyMs=20000,tickAllowance=.005}={}){
 return episodes.map(e=>({...e,marks:horizonsMinutes.map(minutes=>{const target=Date.parse(e.quoteAt)+minutes*60000,series=quotes[e.exchangeId]||[],q=series.find(x=>Date.parse(x.at)>=target);
  if(!q)return {minutes,status:'pending',grossPerShare:null,bufferedPerShare:null};
  if(Date.parse(q.at)>target+maxDelayMs)return {minutes,status:'missing_quote',grossPerShare:null,bufferedPerShare:null};
  if(q.bid===null||q.ask===null||q.bid<=0||q.ask>=1||q.bid>=q.ask||Date.parse(q.startedAt)<target||Date.parse(q.at)-Date.parse(q.startedAt)>maxLatencyMs)return {minutes,status:'unusable_first_quote',grossPerShare:null,bufferedPerShare:null};
  let gross,buffered;try{gross=decimalMicros(String(q.bid))-decimalMicros(String(e.entryAsk));buffered=gross-2n*decimalMicros(String(tickAllowance));}catch{return {minutes,status:'unrepresentable_quote_decimal',grossPerShare:null,bufferedPerShare:null};}
  return {minutes,status:'observed_quote_only',at:q.at,grossMicros:gross.toString(),bufferedMicros:buffered.toString(),grossPerShare:Number(gross)/1e6,bufferedPerShare:Number(buffered)/1e6,proof:q.proof};
 })}));
}
export function runRelativeStudy(inputs,universe,identities,policy=RELATIVE_DIP_POLICY,{candidateCutoffAt=null}={}){
 if(candidateCutoffAt!==null&&(!Number.isFinite(Date.parse(candidateCutoffAt))||new Date(candidateCutoffAt).toISOString()!==candidateCutoffAt))throw Error('RELATIVE_CUTOFF_INVALID');
 const state=createRelativeHistory(universe,identities),episodes=[],lastRace=new Map(),lastMinute=new Map(),quotes={},counts={features:0,ready:0,localDips:0,residualDips:0,reasons:{}};
 for(const {snapshot,proof}of inputs.snapshots){observeRelativePrices(state,snapshot,universe,policy);for(const q of snapshot.quotes){(quotes[q.exchangeId]??=[]).push({at:snapshot.receivedAt,startedAt:snapshot.requestStartedAt,bid:q.bestBid,ask:q.bestAsk,proof});
  if(candidateCutoffAt!==null&&Date.parse(snapshot.receivedAt)<Date.parse(candidateCutoffAt))continue;
  const minute=Math.floor(Date.parse(snapshot.receivedAt)/60000);if(lastMinute.get(q.exchangeId)===minute)continue;lastMinute.set(q.exchangeId,minute);
  const f=relativeDipFeatures(state,q.exchangeId,snapshot.receivedAt,policy);counts.features++;counts.reasons[f.reason]=(counts.reasons[f.reason]||0)+1;if(!f.ready)continue;counts.ready++;if(!f.localDip)continue;counts.localDips++;if(f.residualDip)counts.residualDips++;
  const race=state.identities[q.exchangeId].race,now=Date.parse(f.quoteAt);if(now-(lastRace.get(race)??-Infinity)<policy.episodeSpacingMs)continue;lastRace.set(race,now);episodes.push({...f,race,proof});
 }}
 const start=Date.parse(inputs.snapshots[0]?.snapshot.receivedAt),end=Date.parse(inputs.snapshots.at(-1)?.snapshot.receivedAt),split=start+(end-start)*.7,labels=relativeEpisodeLabels(episodes,quotes);
 const rows=labels.map(e=>({...e,period:candidateCutoffAt!==null?'prospective':Date.parse(e.quoteAt)+900000<split?'development':Date.parse(e.quoteAt)>=split?'subsequent':'purged_boundary'}));
 const summaries=[];for(const period of candidateCutoffAt!==null?['prospective']:['development','subsequent','purged_boundary'])for(const cohort of ['local_dip','market_relative_dip'])for(const horizon of [5,15]){const eligible=rows.filter(e=>e.period===period&&(cohort==='local_dip'||e.residualDip)),marks=eligible.map(e=>e.marks.find(m=>m.minutes===horizon)),observed=marks.filter(m=>m.status==='observed_quote_only');summaries.push({period,cohort,horizonMinutes:horizon,episodes:eligible.length,races:new Set(eligible.map(e=>e.race)).size,observed:observed.length,unobserved:marks.length-observed.length,meanGrossPerShare:observed.length?Number(observed.reduce((s,m)=>s+BigInt(m.grossMicros),0n))/1e6/observed.length:null,meanBufferedPerShare:observed.length?Number(observed.reduce((s,m)=>s+BigInt(m.bufferedMicros),0n))/1e6/observed.length:null,positiveBuffered:observed.filter(m=>BigInt(m.bufferedMicros)>0n).length});}
 return {schemaVersion:1,policy,counts,sources:inputs.sources,snapshotCount:inputs.snapshots.length,candidateCutoffAt,chronologicalSplitAt:candidateCutoffAt!==null?null:Number.isFinite(split)?new Date(split).toISOString():null,rows,summaries,executionAuthority:false,networkRequests:0,ordersSubmitted:0,profitabilityEstablished:false,limitations:['Bulk quotes have no depth or quote-source timestamp; these are recovery labels, not fills or executable P&L.','Party orientation is inferred from explicit cached titles; no resolution equivalence or calibrated factor loading is asserted.',candidateCutoffAt===null?'Fixed unit factor loading and thresholds are hypotheses; this historical diagnostic was designed after prior observations and is not untouched prospective proof.':'Prospective labels use a frozen future cutoff and definition; adequate independent outcomes and executable depth are still required.','Overlapping races and broad events may correlate observations; repeated polling is not independent evidence.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const root=path.dirname(path.dirname(fileURLToPath(import.meta.url))),rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),u=validateUniverse(JSON.parse(fs.readFileSync(path.join(root,'live-paper/price-universe.json'))),JSON.parse(fs.readFileSync(path.join(root,'live-paper/config.json'))).tournamentId),bank=JSON.parse(fs.readFileSync(path.join(rt,'dip-responsive-bank.json')));
 const mode=process.argv[2],name=process.argv[3];if(mode!==undefined&&!['--freeze','--cohort'].includes(mode)||mode!==undefined&&!/^[a-z0-9-]{1,80}$/.test(name||''))throw Error('RELATIVE_COHORT_ARGUMENT');
 const codeFiles=['live-paper/relative-dip.mjs','live-paper/relative-dip-audit.mjs','live-paper/journal-lines.mjs','live-paper/market-price-publication.mjs','live-paper/breadth.mjs','execution/core.mjs'],codeHashes=Object.fromEntries(codeFiles.map(p=>[p,createHash('sha256').update(fs.readFileSync(path.join(root,p))).digest('hex')])),definitionFile=mode?path.join(rt,'relative-dip-cohorts',name,'definition.json'):null;
 let definition=mode==='--cohort'?JSON.parse(fs.readFileSync(definitionFile)):null;
 if(definition&&(fs.readFileSync(definitionFile+'.sha256','utf8').trim()!==hash(definition)||definition.schemaVersion!==1||definition.name!==name||definition.executionAuthority!==false||JSON.stringify(definition.policy)!==JSON.stringify(RELATIVE_DIP_POLICY)||definition.universeHash!==u.hash||JSON.stringify(definition.codeHashes)!==JSON.stringify(codeHashes)||definition.identityBankHash!==hash(bank)))throw Error('RELATIVE_FROZEN_DEFINITION_CHANGED');
 const inputs=verifiedRelativeInputs(['journal.jsonl','market-prices.jsonl'].map(p=>path.join(rt,p)),u,definition?.sourcePrefixes||[]);
 if(mode==='--freeze'){definition={schemaVersion:1,name,cutoffAt:new Date().toISOString(),universeHash:u.hash,identityBankHash:hash(bank),policy:RELATIVE_DIP_POLICY,codeHashes,sourcePrefixes:inputs.sources,executionAuthority:false};fs.mkdirSync(path.dirname(definitionFile),{recursive:true});fs.writeFileSync(definitionFile,JSON.stringify(definition,null,2)+'\n',{flag:'wx'});fs.writeFileSync(definitionFile+'.sha256',hash(definition)+'\n',{flag:'wx'});}
 const report=runRelativeStudy(inputs,u,bank.identities,definition?.policy||RELATIVE_DIP_POLICY,{candidateCutoffAt:definition?.cutoffAt||null});
 report.checkedAt=new Date().toISOString();report.identityBankHash=hash(bank);report.inputIdentityHash=hash(bank.identities);report.definitionHash=definition?hash(definition):null;const output=definition?path.join(path.dirname(definitionFile),'report.json'):path.join(root,'data/relative-dip-live-study.json');fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({...report,rows:undefined}));
}
