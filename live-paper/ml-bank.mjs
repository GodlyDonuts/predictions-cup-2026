import fs from 'node:fs';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {replayJournal} from './replay.mjs';
import {initialState,observe,normalizeBook,invalidate,digest} from './model.mjs';
import {amount,fee,TICK} from '../paper/fixed.mjs';
import {scopedSnapshots} from './depth-scope.mjs';
import {historyPolicy as validateHistoryPolicy,resetHistory,LEGACY,ISOLATED} from './history-policy.mjs';
import {updateContext,contextFeatures} from './breadth-context.mjs';

const num=x=>Number(amount(x))/1e6;
export function features(book,history){
 const mid=(num(book.bids[0].price)+num(book.asks[0].price))/2;
 const bid=book.bids.slice(0,3).reduce((n,l)=>n+l.quantity,0),ask=book.asks.slice(0,3).reduce((n,l)=>n+l.quantity,0);
 const delta=n=>history.length>=n?mid-history[history.length-n].mid:0;
 return [1,num(book.asks[0].price)-num(book.bids[0].price),delta(1),delta(3),delta(6),(book.bids[0].quantity-book.asks[0].quantity)/(book.bids[0].quantity+book.asks[0].quantity),(bid-ask)/(bid+ask),Math.log1p(bid),Math.log1p(ask),(Date.parse(book.receivedAt)-Date.parse(book.sourceAt))/1000,history.length?(Date.parse(book.receivedAt)-Date.parse(history.at(-1).at))/1000:0,mid];
}
export function captureRows(raw,c,{historyPolicy=LEGACY,since=null,breadthContext=false}={}){
 validateHistoryPolicy(historyPolicy);
 if(since!==null&&(typeof since!=='string'||!Number.isFinite(Date.parse(since))))throw Error('Invalid training window');
 const verified=replayJournal(raw,c);let s=null,segment=0,context=null;const histories={},rows=[],marketSegments={};
 const reset=marketId=>{resetHistory(histories,marketId,historyPolicy);if(historyPolicy===LEGACY||marketId===null)segment++;else marketSegments[marketId]=(marketSegments[marketId]||0)+1;};
 for(const line of raw.trimEnd().split('\n')){
  const r=JSON.parse(line),e=r.event;
  if(since!==null&&Date.parse(e.clockAt||r.at)<Date.parse(since))continue;
  if(e.type==='account_read_outage'){reset(null);if(s)invalidate(s,null,e.clockAt||r.at,e.reason);continue;}
  if(e.type!=='cycle')continue;
  for(const snap of scopedSnapshots(e,c)){
   if(snap.skipped){reset(snap.marketId);if(s)invalidate(s,snap.marketId,snap.at||r.at,snap.skipped);continue;}
   const m=/^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint||'');if(!m||!c.marketIds.includes(m[1]))continue;
   if(since!==null&&Date.parse(snap.requestStartedAt)<Date.parse(since))continue;
   s??=initialState(c,'features',snap.requestStartedAt);let b;
   try{b=normalizeBook(snap.data,m[1],c.tournamentId,snap.receivedAt,snap.requestStartedAt,c.maxDataAgeMs,new Set(e.accountExclusions));}catch{reset(m[1]);continue;}
   const prev=s.books[b.instrument];s=observe(s,b,c);if(prev?.versionKey===s.books[b.instrument].versionKey)continue;
   let h=histories[b.instrument]||[];if(h.length&&Date.parse(b.receivedAt)-Date.parse(h.at(-1).at)>120000){if(historyPolicy===ISOLATED)marketSegments[b.marketId]=(marketSegments[b.marketId]||0)+1;else segment++;h=[];}
   const base=features(b,h),x=breadthContext?[...base,...contextFeatures(context,b,c.tournamentId)]:base,row={at:b.receivedAt,marketId:b.marketId,instrument:b.instrument,segment:historyPolicy===ISOLATED?segment+':'+(marketSegments[b.marketId]||0):segment,x,mid:x[11],bid:num(b.bids[0].price),ask:num(b.asks[0].price),book:b};
   rows.push(row);histories[b.instrument]=[...h,row].slice(-30);
  }
  if(breadthContext)context=updateContext(context,e.bulkPrices,e.clockAt||r.at);
 }
 return {rows,sourceHash:verified.replay.sourceHash,sourceRecords:verified.replay.sourceRecords,cutoff:verified.replay.finalEventClock};
}
export function fitRidge(rows,width,lambda){
 if(rows.length<20)throw Error('Insufficient training labels');
 const means=Array.from({length:width},(_,j)=>j?rows.reduce((n,r)=>n+r.x[j],0)/rows.length:0);
 const scales=means.map((m,j)=>j?Math.max(1e-6,Math.sqrt(rows.reduce((n,r)=>n+(r.x[j]-m)**2,0)/rows.length)):1);
 const a=Array.from({length:width},()=>Array(width+1).fill(0));
 for(const r of rows){const x=r.x.slice(0,width).map((v,j)=>(v-means[j])/scales[j]);for(let i=0;i<width;i++){for(let j=0;j<width;j++)a[i][j]+=x[i]*x[j];a[i][width]+=x[i]*r.y;}}
 for(let i=1;i<width;i++)a[i][i]+=lambda;
 for(let i=0;i<width;i++){let p=i;for(let j=i+1;j<width;j++)if(Math.abs(a[j][i])>Math.abs(a[p][i]))p=j;[a[i],a[p]]=[a[p],a[i]];if(Math.abs(a[i][i])<1e-12)throw Error('Singular model');const v=a[i][i];for(let j=i;j<=width;j++)a[i][j]/=v;for(let k=0;k<width;k++)if(k!==i){const f=a[k][i];for(let j=i;j<=width;j++)a[k][j]-=f*a[i][j];}}
 return {width,lambda,means,scales,weights:a.map(r=>r[width])};
}
export function fitStumps(rows,width,iterations,learningRate,minLeaf){
 if(rows.length<20||minLeaf<2||iterations<1||learningRate<=0||learningRate>1)throw Error('Invalid boosted training data');
 const base=rows.reduce((n,r)=>n+r.y,0)/rows.length,predictions=rows.map(()=>base),stumps=[];
 const splits=[];
 for(let feature=1;feature<width;feature++){
  const sorted=rows.map(r=>r.x[feature]).sort((a,b)=>a-b),thresholds=new Set();
  for(let q=1;q<9;q++)thresholds.add(sorted[Math.floor(sorted.length*q/9)]);
  for(const threshold of thresholds){const left=[],right=[];rows.forEach((r,i)=>(r.x[feature]<=threshold?left:right).push(i));if(left.length>=minLeaf&&right.length>=minLeaf)splits.push({feature,threshold,left,right});}
 }
 for(let step=0;step<iterations;step++){
  const residuals=rows.map((r,i)=>r.y-predictions[i]);let best=null;
  for(const split of splits){const l=split.left.reduce((n,i)=>n+residuals[i],0),r=split.right.reduce((n,i)=>n+residuals[i],0),gain=l*l/(split.left.length+10)+r*r/(split.right.length+10);if(!best||gain>best.gain)best={...split,gain,leftValue:learningRate*l/(split.left.length+10),rightValue:learningRate*r/(split.right.length+10)};}
  if(!best||best.gain<1e-15)break;
  stumps.push({feature:best.feature,threshold:best.threshold,leftValue:best.leftValue,rightValue:best.rightValue});
  rows.forEach((r,i)=>predictions[i]+=r.x[best.feature]<=best.threshold?best.leftValue:best.rightValue);
 }
 return {family:'boosted_stumps',width,iterations,learningRate,minLeaf,base,stumps};
}
export const predict=(m,x)=>m.family==='boosted_stumps'?m.stumps.reduce((n,s)=>n+(x[s.feature]<=s.threshold?s.leftValue:s.rightValue),m.base):m.weights.reduce((n,w,j)=>n+w*(x[j]-m.means[j])/m.scales[j],0);
export function instrumentCoverage(train,test,marketIds){
 return marketIds.map(marketId=>{
  const a=train.filter(r=>r.marketId===marketId),b=test.filter(r=>r.marketId===marketId),trainingInstruments=[...new Set(a.map(r=>r.instrument))],testInstruments=[...new Set(b.map(r=>r.instrument))];
  return {marketId,train:a.length,test:b.length,trainingInstruments,testInstruments,eligible:a.length>=8&&b.length>=4&&testInstruments.every(id=>trainingInstruments.includes(id))};
 });
}
export function trainBank(raw,c,{family='ridge',historyPolicy=LEGACY,since=null,requireInstrumentCoverage=false,breadthContext=false}={}){
 if(!['ridge','boosted_stumps'].includes(family))throw Error('Unsupported model family');
 if(typeof requireInstrumentCoverage!=='boolean')throw Error('Invalid coverage requirement');
 if(typeof breadthContext!=='boolean')throw Error('Invalid breadth feature selection');
 const source=captureRows(raw,c,{historyPolicy,since,breadthContext}),rows=source.rows;
 if(rows.length<80)throw Error('Need at least 80 accepted snapshots before model fitting');
 const split=rows[Math.floor(rows.length*.65)].at,models=[],rejected=[];
 for(const horizon of [60,180,300,600]){
  const labels=[];
  for(let i=0;i<rows.length;i++){
   const r=rows[i];let future;
   for(let j=i+1;j<rows.length;j++){const f=rows[j];if(f.instrument!==r.instrument)continue;if(f.segment!==r.segment)break;const dt=Date.parse(f.at)-Date.parse(r.at);if(dt>=horizon*1000){if(dt<=horizon*1000+120000)future=f;break;}}
   if(!future)continue;
   const exit=amount(future.book.bids[0].price)-TICK*BigInt(c.slippageTicks);if(exit<=0n)continue;
   const currentExit=amount(r.book.bids[0].price)-TICK*BigInt(c.slippageTicks);
   const persistence=Number(currentExit-fee(currentExit,1,{feeBps:c.feeBps,feePerShare:'0.000000'}))/1e6-r.mid;
   labels.push({...r,labelAt:future.at,persistence,y:Number(exit-fee(exit,1,{feeBps:c.feeBps,feePerShare:'0.000000'}))/1e6-r.mid});
  }
  const train=labels.filter(r=>Date.parse(r.labelAt)<Date.parse(split)),test=labels.filter(r=>Date.parse(r.at)>=Date.parse(split));
  const coverage=instrumentCoverage(train,test,c.marketIds);
  const variants=[];
  for(const width of (breadthContext?[12,15,17,19]:[4,6,9,12])){
   if(family==='ridge')for(const lambda of [.01,.1,1,10,100,1000,10000,100000])variants.push({width,lambda,id:`ridge-h${horizon}-f${width}-l${lambda}`});
   else for(const iterations of [16,64])for(const learningRate of [.05,.1])for(const minLeaf of [5,10])variants.push({width,iterations,learningRate,minLeaf,id:`stumps-h${horizon}-f${width}-n${iterations}-r${learningRate}-leaf${minLeaf}`});
  }
  for(const {id,width,lambda,iterations,learningRate,minLeaf} of variants){
   if(train.length<20||test.length<10){rejected.push({id,reason:'insufficient_matured_labels',train:train.length,test:test.length});continue;}
   if(requireInstrumentCoverage&&coverage.some(x=>!x.eligible)){rejected.push({id,reason:'insufficient_instrument_coverage',coverage});continue;}
   const model=family==='ridge'?fitRidge(train,width,lambda):fitStumps(train,width,iterations,learningRate,minLeaf);
   const errors=test.map(r=>predict(model,r.x)-r.y),rmse=Math.sqrt(errors.reduce((n,v)=>n+v*v,0)/errors.length),baselineRmse=Math.sqrt(test.reduce((n,r)=>n+(r.y-r.persistence)**2,0)/test.length);
   models.push({id,horizon,...model,coverage,trainRows:train.length,testRows:test.length,rmse,baselineRmse,baseline:'Current bid persists, with the same assumed exit tick and fee',beatsPersistence:rmse<baselineRmse,maxTrainingLabelAt:new Date(Math.max(...train.map(r=>Date.parse(r.labelAt)))).toISOString()});
  }
 }
 const selected=[60,180,300,600].flatMap(h=>models.filter(m=>m.horizon===h).sort((a,b)=>a.rmse-b.rmse).slice(0,3));
 return {schemaVersion:1,family,historyPolicy,trainingWindowStart:since,requireInstrumentCoverage,breadthContext,createdAt:new Date().toISOString(),config:c,configHash:digest(c),sourceHash:source.sourceHash,sourceRecords:source.sourceRecords,trainingCaptureThrough:source.cutoff,developmentSplitAt:split,acceptedSnapshots:rows.length,candidatesAttempted:128,modelsTrained:models.length,models,rejected,selected,selection:'Three lowest development-split RMSE per horizon; exploration only, all tried variants retained',target:'Future haircut-free best bid minus adverse exit tick and assumed exit fee, relative to current midpoint; entry costs charged separately',limitations:['Short correlated capture; no profitability established','Chronological development split with training labels purged at split; not untouched future validation','Top bid label is not verified full-size exit liquidity','No guarantee of future depth, latency, settlement equivalence or execution']};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const arg=n=>{const i=process.argv.indexOf(n);if(i<0||!process.argv[i+1])throw Error('Required '+n);return process.argv[i+1];};const family=process.argv.includes('--family')?arg('--family'):'ridge';const bank=trainBank(fs.readFileSync(arg('--journal'),'utf8'),JSON.parse(fs.readFileSync(arg('--config'),'utf8')),{family,historyPolicy:process.argv.includes('--history-policy')?arg('--history-policy'):LEGACY,since:process.argv.includes('--since')?arg('--since'):null,requireInstrumentCoverage:process.argv.includes('--require-instrument-coverage'),breadthContext:process.argv.includes('--breadth-context')});const out=arg('--out');if(fs.existsSync(out))throw Error('Refusing to overwrite an existing frozen model bank');const tmp=out+'.'+randomUUID()+'.tmp';fs.writeFileSync(tmp,JSON.stringify(bank,null,2),{flag:'wx',mode:0o600});try{fs.linkSync(tmp,out);}finally{fs.unlinkSync(tmp);}console.log(JSON.stringify({attempted:bank.candidatesAttempted,trained:bank.modelsTrained,shadowModels:bank.selected.length,snapshots:bank.acceptedSnapshots,cutoff:bank.trainingCaptureThrough}));
}
