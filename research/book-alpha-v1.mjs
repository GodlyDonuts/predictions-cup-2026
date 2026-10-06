// Offline research only. Floating regression outputs never serve as a ledger.
// Fixed before capture: pooled ridge200, one-share/1% depth, protective1tick,
// independent later receipts, three horizons, chronological60% clock split.
export const BOOK_ALPHA_POLICY=Object.freeze({horizonsMs:[30000,120000,300000],ridge:200,trainFraction:0.6,latencyMs:1000,maximumLatenessMs:30000,minimumNet:0.005,tick:0.005,depthFractionBps:100,fees:0});
const need=(v,c)=>{if(!v)throw Error('BOOK_ALPHA_'+c);};
export function resetBookEpochs(active,row){if(row.kind==='book')return false;if(row.kind==='invalidation'&&row.payload.marketId!==null)active.delete(row.payload.marketId);else active.clear();return true;}
export function bookFeatures(book,prior){
 const bid=Number(book.bids[0]?.price),ask=Number(book.asks[0]?.price);if(!Number.isFinite(bid)||!Number.isFinite(ask)||bid<0.005||ask>0.995||bid>ask)return null;
 const bq=book.bids.slice(0,5).reduce((n,l)=>n+Math.min(l.quantity,1e9),0),aq=book.asks.slice(0,5).reduce((n,l)=>n+Math.min(l.quantity,1e9),0);if(bq+aq<=0)return null;
 const mid=(bid+ask)/2,imbalance=(bq-aq)/(bq+aq),pastChange=prior?mid-prior.mid:0;
 return{bid,ask,mid,features:[ask-bid,imbalance,imbalance*(ask-bid)/2,pastChange,mid*(1-mid)],buyCapacity:Math.floor(book.asks[0].quantity/100),sellCapacity:Math.floor(book.bids[0].quantity/100)};
}
function firstAfter(rows,at){let lo=0,hi=rows.length;while(lo<hi){const m=(lo+hi)>>1;if(rows[m].requestAt<at)lo=m+1;else hi=m;}return rows[lo];}
export function causalBookLabels(streams,horizon,policy=BOOK_ALPHA_POLICY){
 const labels=[];for(const rows of streams){for(let i=0;i<rows.length;i++){const r=rows[i],buy=firstAfter(rows,r.at+policy.latencyMs),exit=firstAfter(rows,r.at+horizon);
  if(!buy||!exit||buy.sequence<=r.sequence||exit.sequence<=buy.sequence||buy.requestAt<=r.at||exit.requestAt<buy.at+policy.latencyMs||buy.requestAt-r.at>policy.maximumLatenessMs||exit.requestAt-r.at-horizon>policy.maximumLatenessMs||buy.buyCapacity<1||exit.sellCapacity<1||buy.ask>r.ask+policy.tick+1e-12)continue;
  labels.push({...r,labelAt:exit.at,target:exit.bid-r.bid,observedAcquisition:buy.ask,observedLiquidation:exit.bid,entryAt:buy.at,exitAt:exit.at});
 }}return labels.sort((a,b)=>a.at-b.at||a.marketId.localeCompare(b.marketId));
}
function solve(matrix,vector){const n=vector.length,a=matrix.map((r,i)=>[...r,vector[i]]);for(let p=0;p<n;p++){let max=p;for(let i=p+1;i<n;i++)if(Math.abs(a[i][p])>Math.abs(a[max][p]))max=i;[a[p],a[max]]=[a[max],a[p]];need(Math.abs(a[p][p])>1e-12,'SINGULAR');const d=a[p][p];for(let j=p;j<=n;j++)a[p][j]/=d;for(let i=0;i<n;i++)if(i!==p){const f=a[i][p];for(let j=p;j<=n;j++)a[i][j]-=f*a[p][j];}}return a.map(r=>r[n]);}
export function fitBookAlpha(labels,cutoff,policy=BOOK_ALPHA_POLICY){
 const train=labels.filter(r=>r.at<cutoff&&r.labelAt<=cutoff);need(train.length>=20,'INSUFFICIENT_CHRONOLOGY');const dimensions=train[0].features.length;
 const mean=Array.from({length:dimensions},(_,j)=>train.reduce((n,r)=>n+r.features[j],0)/train.length),scale=mean.map((m,j)=>Math.sqrt(train.reduce((n,r)=>n+(r.features[j]-m)**2,0)/train.length)||1),n=dimensions+1,matrix=Array.from({length:n},()=>Array(n).fill(0)),vector=Array(n).fill(0);
 for(const r of train){const x=[1,...r.features.map((v,j)=>(v-mean[j])/scale[j])];for(let i=0;i<n;i++){vector[i]+=x[i]*r.target;for(let j=0;j<n;j++)matrix[i][j]+=x[i]*x[j];}}
 for(let i=0;i<n;i++)matrix[i][i]+=policy.ridge;const coefficients=solve(matrix,vector);return{cutoff,trainLabels:train.length,mean,scale,coefficients,policy};
}
export const predictBookAlpha=(model,row)=>model.coefficients[0]+row.features.reduce((n,v,j)=>n+model.coefficients[j+1]*(v-model.mean[j])/model.scale[j],0);
export function scoreBookAlpha(model,labels,decisions=labels){
 const later=labels.filter(r=>r.at>=model.cutoff),outcomes=new Map(later.map(r=>[r.marketId+':'+r.sequence,r])),selected=[];let modelSquare=0,baselineSquare=0;
 for(const r of later){const prediction=predictBookAlpha(model,r);modelSquare+=(prediction-r.target)**2;baselineSquare+=r.target**2;}
 const available=decisions.filter(r=>r.at>=model.cutoff);for(const r of available){const prediction=predictBookAlpha(model,r),predictedNet=r.bid+prediction-r.ask-model.policy.tick;if(predictedNet>=model.policy.minimumNet){const outcome=outcomes.get(r.marketId+':'+r.sequence);selected.push({marketId:r.marketId,at:new Date(r.at).toISOString(),predictedNet,laterReceiptOutcomeAvailable:!!outcome,observedRawQuoteMargin:outcome?outcome.observedLiquidation-outcome.observedAcquisition:null,oneTickPerSideMargin:outcome?outcome.observedLiquidation-outcome.observedAcquisition-2*model.policy.tick:null});}}
 return{laterLabels:later.length,laterDecisionObservations:available.length,rmse:later.length?Math.sqrt(modelSquare/later.length):null,unchangedBidRmse:later.length?Math.sqrt(baselineSquare/later.length):null,selectedQuoteSignals:selected.length,censoredSelectedSignals:selected.filter(x=>!x.laterReceiptOutcomeAvailable).length,selected,accountRequests:0,acceptedOrders:0,realizedPnl:null,profitabilityEstablished:false};
}
