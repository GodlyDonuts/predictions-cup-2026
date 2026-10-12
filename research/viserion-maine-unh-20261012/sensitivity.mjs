import fs from 'node:fs';
import crypto from 'node:crypto';
import {probability} from '../poll-calibration-v1/model.mjs';
import {referencePassivePlanV2} from '../../paper/reference-passive-plan-v2.mjs';
const root=new URL('../../',import.meta.url),read=p=>{const bytes=fs.readFileSync(new URL(p,root));return{path:p,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),value:JSON.parse(bytes)};};
const old=read('research/maine-primary-screen-20261012/sources.json'),calibration=read('research/maine-primary-screen-20261012/average-calibration.json'),addition=read('research/viserion-maine-unh-20261012/sources.json'),bank=read('data/maine-passive-harvest-paper-20261012-bank.json'),state=read('data/maine-passive-harvest-paper-20261012-state.json');
const s=addition.value.siena,b=bank.value,instrument=b.instruments[0],book=state.value.books[instrument.marketId];
if(old.value.captures.some(p=>p.id===s.id))throw Error('DUPLICATE_SOURCE');
if(s.democraticVoteSharePercent-s.republicanVoteSharePercent!==s.democraticMargin||!s.fieldEnd||s.numericUse!=='conditional source-completeness sensitivity only')throw Error('SOURCE_FACTS');
const asOf=calibration.value.at;
function aggregate(polls,halfLife){
 const ps=polls.map(p=>{const ageDays=(Date.parse(asOf)-Date.parse(p.fieldEnd+'T23:59:59.999Z'))/86400000;if(ageDays<0||ageDays>30)throw Error('SOURCE_OUTSIDE_DECLARED_WINDOW');return{id:p.id,margin:p.democraticMargin,weight:2**(-ageDays/halfLife),ageDays};});
 const denominator=ps.reduce((n,p)=>n+p.weight,0);return{polls:ps,margin:ps.reduce((n,p)=>n+p.margin*p.weight,0)/denominator};
}
function plan(value){return referencePassivePlanV2({book,side:instrument.side,minimumGap:instrument.minimumGap,sizing:{equityLower:b.initialCash,cashAvailable:state.value.cash,referenceValue:(Math.floor(value*1e6)/1e6).toFixed(6),currentQuantity:0,reservedBuyQuantity:0,depthQuantity:2147483647,kellyFractionBps:b.kellyFractionBps,eventFractionBps:b.eventFractionBps,eventPrincipal:'0.000000',grossRemaining:b.capitalLimit}});}
const rows=calibration.value.rows.map(row=>{
 const original=aggregate(old.value.captures,row.halfLifeDays),extended=aggregate([...old.value.captures,s],row.halfLifeDays);
 const originalValue=probability(row.model,original.margin)-.02,extendedValue=probability(row.model,extended.margin)-.02;
 if(Math.abs(originalValue-row.currentRepublicanNoCentralValue)>1e-12||Math.abs(original.margin-row.currentWeightedDemocraticMargin)>1e-12)throw Error('ORIGINAL_REFERENCE_REPRODUCTION_FAILED');
 return{halfLifeDays:row.halfLifeDays,declaredDefault:row.halfLifeDays===14,unchangedModel:row.model,original:{...original,referenceValue:originalValue,quotePlan:plan(originalValue)},withSiena:{...extended,referenceValue:extendedValue,threePointAdverseValue:probability(row.model,extended.margin-3)-.02,quotePlan:plan(extendedValue)},referenceValueChange:extendedValue-originalValue};
});
const output={at:new Date().toISOString(),mode:'missing_contrary_primary_source_fixed_model_sensitivity',originalReferenceAt:asOf,rows,inputSources:[old,calibration,addition,bank,state].map(({path,sha256})=>({path,sha256})),historicalBookAt:book.receivedAt,currentExecutableBookVerified:false,
 originalBankChanged:false,modelRefitted:false,sourceCensusComplete:false,unhNumericIncluded:false,populationTransportValidated:false,questionSpecificSienaPopulationUnresolved:true,valuesAreNotConfidenceBounds:true,winProbabilitiesAreNotShortTermPriceForecasts:true,parametersSelectedByScore:false,marketProfitabilityEstablished:false,actualOrders:0,accountRequests:0};
fs.writeFileSync(new URL('results.json',import.meta.url),JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify(rows.map(r=>({halfLife:r.halfLifeDays,originalValue:r.original.referenceValue,withSiena:r.withSiena.referenceValue,originalQuantity:r.original.quotePlan.quantity,withSienaQuantity:r.withSiena.quotePlan.quantity,reason:r.withSiena.quotePlan.reason})),null,2));
