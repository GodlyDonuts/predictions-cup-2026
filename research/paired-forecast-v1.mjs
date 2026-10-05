// Predict future liquidation changes, with persistence as the shrinkage prior.
// Source quotes do not contain depth; this module never produces fills.
export const DEFINITION=Object.freeze({horizonsMs:[300000,1800000,7200000],maximumLabelDelayMs:60000,ridgePenalty:1000,minimumTrainingRows:1000,minimumPredictedNet:0.01,trainingFraction:0.6,featureNames:['discount_to_one','quoted_roundtrip_cost','lag_five_minute_change','lag_unavailable']});
const need=(v,c)=>{if(!v)throw Error('PAIR_FORECAST_'+c);};
export function causalFeatures(rows,index){const r=rows[index];let j=index-1;while(j>=0&&rows[j].at>r.at-300000)j--;const lag=j>=0&&r.at-rows[j].at<=600000?rows[j]:null;
 return[1-(r.buy+r.sell)/2,r.buy-r.sell,lag?r.sell-lag.sell:0,lag?0:1];}
export function maturedLabels(rows,horizon,cutoff=Infinity){const out=[];let j=0;for(let i=0;i<rows.length;i++){const r=rows[i];j=Math.max(j,i+1);while(j<rows.length&&rows[j].at<r.at+horizon)j++;if(j===rows.length)break;const future=rows[j];if(future.at>cutoff||future.at>r.at+horizon+DEFINITION.maximumLabelDelayMs||future.minRequestAt<r.at+horizon)continue;out.push({pair:r.pair,at:r.at,labelAt:future.at,x:causalFeatures(rows,i),currentBuy:r.buy,currentSell:r.sell,y:future.sell-r.sell,net:future.sell-r.buy});}return out;}
function solve(a,b){const n=b.length,m=a.map((r,i)=>[...r,b[i]]);for(let k=0;k<n;k++){let p=k;for(let i=k+1;i<n;i++)if(Math.abs(m[i][k])>Math.abs(m[p][k]))p=i;[m[k],m[p]]=[m[p],m[k]];need(Math.abs(m[k][k])>1e-12,'SINGULAR');const v=m[k][k];for(let j=k;j<=n;j++)m[k][j]/=v;for(let i=0;i<n;i++)if(i!==k){const f=m[i][k];for(let j=k;j<=n;j++)m[i][j]-=f*m[k][j];}}return m.map(r=>r[n]);}
export function fitFrozenForecast(labels,cutoff){const train=labels.filter(r=>r.labelAt<=cutoff);need(train.length>=DEFINITION.minimumTrainingRows,'INSUFFICIENT_TRAINING');const d=DEFINITION.featureNames.length,mean=Array(d).fill(0),scale=Array(d).fill(0);
 for(const r of train)for(let j=0;j<d;j++)mean[j]+=r.x[j]/train.length;
 for(const r of train)for(let j=0;j<d;j++)scale[j]+=(r.x[j]-mean[j])**2/train.length;
 for(let j=0;j<d;j++)scale[j]=Math.max(1e-6,Math.sqrt(scale[j]));
 const a=Array.from({length:d+1},()=>Array(d+1).fill(0)),b=Array(d+1).fill(0);
 for(const r of train){const x=[1,...r.x.map((v,j)=>(v-mean[j])/scale[j])];for(let i=0;i<=d;i++){b[i]+=x[i]*r.y;for(let j=0;j<=d;j++)a[i][j]+=x[i]*x[j];}}
 // Intercept is penalized too: an unconditional historical drift is not alpha.
 for(let i=0;i<=d;i++)a[i][i]+=DEFINITION.ridgePenalty;
 return{schemaVersion:1,cutoff,definition:DEFINITION,trainingRows:train.length,maximumTrainingLabelAt:train.reduce((v,r)=>Math.max(v,r.labelAt),-Infinity),mean,scale,coefficients:solve(a,b),stateSpecificPredictors:false};}
export function forecastChange(model,x){return model.coefficients[0]+x.reduce((s,v,j)=>s+model.coefficients[j+1]*(v-model.mean[j])/model.scale[j],0);}
export function scoreForecast(model,labels){const later=labels.filter(r=>r.at>model.cutoff);let sq=0,baseSq=0,selected=0,net=0;const byDay={};
 for(const r of later){const change=forecastChange(model,r.x),predictedNet=r.currentSell+change-r.currentBuy;sq+=(change-r.y)**2;baseSq+=r.y**2;const day=new Date(r.at).toISOString().slice(0,10),g=byDay[day]??={observations:0,selectedQuoteSignals:0,selectedQuoteNetSum:0};g.observations++;if(predictedNet>=DEFINITION.minimumPredictedNet){selected++;net+=r.net;g.selectedQuoteSignals++;g.selectedQuoteNetSum+=r.net;}}
 return{laterRows:later.length,rmse:later.length?Math.sqrt(sq/later.length):null,persistenceRmse:later.length?Math.sqrt(baseSq/later.length):null,selectedQuoteSignals:selected,selectedQuoteNetSum:net,selectedQuoteNetMean:selected?net/selected:null,byDay,simulatedFills:0,realizedPnl:null,executionEvidence:false,profitabilityEstablished:false};}
