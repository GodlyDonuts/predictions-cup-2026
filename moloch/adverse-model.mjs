import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {ActivityStudy} from './activity.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {verifyJournal} from './compare.mjs';

const check=(ok,code)=>{if(!ok)throw Error('ADVERSE_'+code);};
export const FEATURE_NAMES=['spreadTicks','sideImbalance','entryPrice','logOwnQueue','logOppositeQueue','receiptLagSeconds','sideMomentumTicks','volatilityTicks'];
export function quoteFeatures(snapshot,side,history=[]){
  check(side==='YES'||side==='NO','SIDE');
  const b=snapshot.book,bid=b.bids[0],ask=b.asks[0],sign=side==='YES'?1:-1;
  const values=[Math.round((ask.price-bid.price)*200),sign*(bid.quantity-ask.quantity)/(bid.quantity+ask.quantity),side==='YES'?bid.price:1-ask.price,Math.log1p(side==='YES'?bid.quantity:ask.quantity),Math.log1p(side==='YES'?ask.quantity:bid.quantity),(Date.parse(snapshot.receivedAt)-Date.parse(b.asOf.at))/1000];
  const cutoff=Date.parse(snapshot.receivedAt),past=history.filter(h=>h.generation===snapshot.generation&&h.book.exchangeId===b.exchangeId&&Date.parse(h.receivedAt)>=cutoff-30000&&Date.parse(h.receivedAt)<=cutoff&&h.book.asOf.sequence<=b.asOf.sequence).sort((a,b)=>Date.parse(a.receivedAt)-Date.parse(b.receivedAt));
  const mid=h=>(h.book.bids[0].price+h.book.asks[0].price)*100,current=mid(snapshot);
  values.push(past.length?sign*(current-mid(past[0])):0,past.length>1?past.slice(1).reduce((n,h,i)=>n+Math.abs(mid(h)-mid(past[i])),0)/(past.length-1):0);
  check(values.every(Number.isFinite)&&bid.quantity>0&&ask.quantity>0&&values[5]>=0,'FEATURES');return values;
}

export function extractRows(rows){
  const study=new ActivityStudy(),features=new Map(),labels=new Map();
  for(const row of rows){
    const e=row.event,m=study.markets.get(e.marketId),beforeMarks=m?.markouts.length??0;
    // Freeze features before this batch can contribute its trades or books.
    if(m&&e.kind==='broadcast'&&e.event==='market_batch'&&!e.gap&&!e.duplicate&&!e.payload.resyncRequired){
      for(const t of e.payload.trades){
        if(t.id===null||t.sequence===null)continue;
        const executed=Date.parse(t.executedAt),history=m.history.get(t.exchangeId)??[];
        const prior=[...history].reverse().find(b=>b.generation===m.generation&&b.book.asOf.sequence<t.sequence&&Date.parse(b.receivedAt)<=executed-3000&&Date.parse(b.book.asOf.at)<=executed&&executed-Date.parse(b.book.asOf.at)<=10000);
        if(!prior)continue;
        const side=t.price<=prior.book.bids[0].price?'YES':t.price>=prior.book.asks[0].price?'NO':null;
        if(!side)continue;
        const key=[t.exchangeId,t.id].join(':');
        if(!features.has(key))features.set(key,{side,quoteAt:prior.receivedAt,bookHash:payloadHash(prior.book),quoteTicks:Math.round((side==='YES'?prior.book.bids[0].price:prior.book.asks[0].price)*200),features:quoteFeatures(prior,side,history)});
      }
    }
    study.row(row); // Reject edited chains before returning any sample.
    if(e.kind==='broadcast'&&e.event==='market_batch')for(const mark of study.markets.get(e.marketId).markouts.slice(beforeMarks)){
      const b=e.payload.books.find(b=>b.exchangeId===mark.exchangeId&&b.asOf.sequence>=mark.sequence&&Date.parse(b.asOf.at)>=mark.dueAt&&Date.parse(b.asOf.at)<=mark.dueAt+10000&&Date.parse(e.receivedAt)-Date.parse(b.asOf.at)>=0&&Date.parse(e.receivedAt)-Date.parse(b.asOf.at)<=10000&&Math.round(((mark.side==='YES'?b.bids[0]?.price:b.asks[0]?.price)*200-mark.quoteTicks)*(mark.side==='YES'?1:-1))===Math.round(Number(mark.liquidationDeltaPerShare)*200));
      if(b)labels.set([mark.exchangeId,mark.tradeId].join(':'),{labelSourceAt:b.asOf.at,labelBookHash:payloadHash(b)});
    }
  }
  const samples=[];
  for(const m of study.markets.values())for(const mark of m.markouts){
    const f=features.get([mark.exchangeId,mark.tradeId].join(':'));
    const label=labels.get([mark.exchangeId,mark.tradeId].join(':'));
    if(!f||!label||f.side!==mark.side||f.quoteTicks!==mark.quoteTicks)continue;
    samples.push({...f,...label,marketId:m.marketId,exchangeId:mark.exchangeId,tradeId:mark.tradeId,executedAt:mark.executedAt,labelAt:mark.markedAt,delta:Number(mark.liquidationDeltaPerShare),label:Number(Number(mark.liquidationDeltaPerShare)<0)});
  }
  return{samples,eventCount:study.sequence,finalHash:study.previous,censored:[...study.markets.values()].reduce((n,m)=>n+m.pending.length,0),excludedMarks:[...study.markets.values()].reduce((n,m)=>n+m.markouts.length,0)-samples.length};
}

export function chronologicalSplit(samples,trainThrough,testFrom){
  const cutoff=Date.parse(trainThrough),start=Date.parse(testFrom);check(Number.isFinite(cutoff)&&Number.isFinite(start)&&start>cutoff,'SPLIT');
  return{train:samples.filter(s=>Date.parse(s.quoteAt)<cutoff&&Date.parse(s.labelAt)<=cutoff),test:samples.filter(s=>Date.parse(s.quoteAt)>=start)};
}
const sigmoid=z=>1/(1+Math.exp(-Math.max(-40,Math.min(40,z))));
export function predictAdverse(model,features){
  check(features.length===model.featureNames.length&&features.every(Number.isFinite)&&model.weights.length===features.length+1&&model.mean.length===features.length&&model.scale.length===features.length,'PREDICTION');
  return sigmoid(model.weights[0]+features.reduce((n,x,j)=>n+model.weights[j+1]*(x-model.mean[j])/model.scale[j],0));
}
export function fitAdverse(samples){
  check(samples.length>0&&samples.every(s=>[0,1].includes(s.label)&&s.features.length===FEATURE_NAMES.length&&s.features.every(Number.isFinite)),'TRAIN');
  const n=samples.length,k=FEATURE_NAMES.length,rate=samples.reduce((n,s)=>n+s.label,0)/n;
  const mean=FEATURE_NAMES.map((_,j)=>samples.reduce((n,s)=>n+s.features[j],0)/n);
  const scale=mean.map((v,j)=>Math.sqrt(samples.reduce((n,s)=>n+(s.features[j]-v)**2,0)/n)||1);
  const model={schemaVersion:1,featureNames:FEATURE_NAMES,mean,scale,weights:[Math.log((rate*n+1)/((1-rate)*n+1)),...Array(k).fill(0)],trainingRate:rate,epochs:400,learningRate:.1,ridge:.01};
  for(let step=0;step<model.epochs;step++){
    const gradient=Array(k+1).fill(0);
    for(const s of samples){const error=predictAdverse(model,s.features)-s.label;gradient[0]+=error;for(let j=0;j<k;j++)gradient[j+1]+=error*(s.features[j]-mean[j])/scale[j];}
    model.weights=model.weights.map((w,j)=>w-model.learningRate*(gradient[j]/n+(j?model.ridge*w:0)));
  }
  return model;
}
export function evaluateAdverse(model,samples){
  if(!samples.length)return{samples:0,brier:null,baselineBrier:null};
  const predictions=samples.map(s=>({...s,p:predictAdverse(model,s.features)})),selected=predictions.filter(s=>s.p<=.10);
  return{samples:samples.length,adverseRate:samples.reduce((n,s)=>n+s.label,0)/samples.length,brier:predictions.reduce((n,s)=>n+(s.p-s.label)**2,0)/samples.length,baselineBrier:samples.reduce((n,s)=>n+(model.trainingRate-s.label)**2,0)/samples.length,fixedGate:{maximumPredictedAdverse:.10,samples:selected.length,adverseRate:selected.length?selected.reduce((n,s)=>n+s.label,0)/selected.length:null,meanConditionalDelta:selected.length?selected.reduce((n,s)=>n+s.delta,0)/selected.length:null},profitabilityEstablished:false};
}
export function runStudy(config){
  const sources=[],samples=[],seen=new Set();
  for(const directory of config.episodes){
    const read=name=>JSON.parse(fs.readFileSync(path.join(directory,name),'utf8')),stats=read('stats.json'),snapshot=read('state.json'),{hash,...body}=snapshot;
    check(stats.status==='completed'&&stats.captureExit===0&&hash===payloadHash(body),'TERMINAL');
    verifyJournal(fs.readFileSync(path.join(directory,'events.jsonl'),'utf8').trim().split(/\r?\n/).map(JSON.parse),snapshot);
    const bytes=fs.readFileSync(path.join(directory,'capture','stream','events.jsonl')),result=extractRows(bytes.toString('utf8').trim().split(/\r?\n/).map(JSON.parse));
    sources.push({episode:path.basename(directory),sha256:createHash('sha256').update(bytes).digest('hex'),eventCount:result.eventCount,finalHash:result.finalHash,censored:result.censored,excludedMarks:result.excludedMarks});
    for(const sample of result.samples){const key=[sample.exchangeId,sample.tradeId].join(':');if(!seen.has(key)){seen.add(key);samples.push(sample);}}
  }
  const split=chronologicalSplit(samples,config.trainThrough,config.testFrom),model=fitAdverse(split.train);
  return{schemaVersion:1,bot:'Moloch',target:'Adverse 30–40-second executable one-share mark conditional on an observed compatible print; not actual fill probability.',labelWindowMs:{from:30000,to:40000},sources,split:{trainThrough:config.trainThrough,testFrom:config.testFrom,train:split.train.length,test:split.test.length},model,modelHash:payloadHash(model),training:evaluateAdverse(model,split.train),evaluation:evaluateAdverse(model,split.test),contestOrders:0,profitabilityEstablished:false,limitations:['Features are from a prior received book at least three seconds before execution, with source age at most ten seconds. No later print quantity, price or touch status is a model input.','Future label books must have source times 30–40 seconds after execution and receipt lag at most ten seconds. Stale and later-horizon marks are excluded.','Training includes only labels available by its cutoff; normalization and coefficients use training only. Hyperparameters and the .10 gate are fixed.','Historical evaluation periods were previously inspected; this is retrospective and is not pristine prospective evidence.','Overlapping labels and markets are correlated. Sample count is not an independent-trial count.','Missing future marks remain censored; stream gaps and incomplete observation can bias the sample.','A favorable prediction score or conditional price mark does not establish queue fills, wallet profit or deployment authority.']};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){const result=runStudy(JSON.parse(fs.readFileSync(process.argv[2],'utf8')));fs.writeFileSync(process.argv[3],JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({split:result.split,training:result.training,evaluation:result.evaluation}));}
