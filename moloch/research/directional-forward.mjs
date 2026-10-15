import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {payloadHash} from '../../execution/ledger.mjs';
import {ActivityStudy} from '../activity.mjs';

const decimal = n => `${n < 0n ? '-' : ''}${(n < 0n ? -n : n)/1000000n}.${((n < 0n ? -n : n)%1000000n).toString().padStart(6,'0')}`;
const micros = s => { if(!/^-?\d+\.\d{6}$/.test(s))throw Error('MARK_DECIMAL'); return BigInt(s.replace('.','')); };

export function evaluateDirectional(registration, study) {
  const cutoff=Date.parse(registration.registeredAt), lead=registration.minimumPriorObservableLeadMs;
  if(!Number.isFinite(cutoff)||!Number.isSafeInteger(lead)||lead<3000||!['YES','NO'].includes(registration.side)||registration.markoutMs!==30000||registration.fundedWallet!==false||registration.contestOrders!==0)throw Error('DIRECTIONAL_REGISTRATION');
  const market=study.markets.get(registration.marketId);
  // A hypothetical quote cannot activate before registration plus its latency.
  const eligible=s=>s.exchangeId===registration.exchangeId&&s.side===registration.side&&Date.parse(s.executedAt)>cutoff+lead;
  const completed=(market?.markouts??[]).filter(eligible);
  const censored=(market?.pending??[]).filter(eligible).map(s=>({...s,reason:s.generation===market.generation?'later qualifying book not yet observed':'source gap before qualifying mark'}));
  let sum=0n,weighted=0n,quantity=0n;
  for(const s of completed){if(!Number.isSafeInteger(s.quantity)||s.quantity<=0)throw Error('MARK_QUANTITY');const p=micros(s.liquidationDeltaPerShare);sum+=p;weighted+=p*BigInt(s.quantity);quantity+=BigInt(s.quantity);}
  return {schemaVersion:1,registrationHash:payloadHash(registration),registeredAt:registration.registeredAt,sourceEventCount:study.sequence,sourceFinalHash:study.previous,window:{from:study.firstAt,to:study.lastAt},marketId:registration.marketId,exchangeId:registration.exchangeId,side:registration.side,completedSamples:completed.length,censoredSamples:censored.length,distinctThirtySecondBuckets:new Set(completed.map(s=>Math.floor(Date.parse(s.executedAt)/30000))).size,meanConditionalDelta:completed.length?decimal(sum/BigInt(completed.length)):null,volumeWeightedConditionalDelta:quantity?decimal(weighted/quantity):null,completed,censored,fundedWallet:false,contestOrders:0,profitabilityEstablished:false,limitations:['Only executions after registration plus three-second activation are included.','Conditional quotes and overlapping marks are not actual fills or independent return observations.','Missing later marks remain censored; favorable marks do not establish funded wallet profit.']};
}

if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
  const [registrationPath,input,output]=process.argv.slice(2);
  if(!registrationPath||!input||!output)throw Error('registration, frozen stream prefix and new output required');
  const registration=JSON.parse(fs.readFileSync(registrationPath)),raw=fs.readFileSync(input,'utf8');
  if(!raw.endsWith('\n'))throw Error('INCOMPLETE_STREAM_PREFIX');
  const study=new ActivityStudy();for(const line of raw.split(/\r?\n/).filter(Boolean))study.row(JSON.parse(line));
  const result={...evaluateDirectional(registration,study),sourceSha256:createHash('sha256').update(raw).digest('hex')};
  fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
  console.log(JSON.stringify({...result,completed:undefined,censored:undefined},null,2));
}
