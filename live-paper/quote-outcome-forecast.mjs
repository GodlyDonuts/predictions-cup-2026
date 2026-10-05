import {MICROPRICE_FEATURES} from './realtime-microprice-dataset.mjs';
const need=(v,c)=>{if(!v)throw Error('QUOTE_FORECAST_'+c);};
function predict(model,x,outputs){const n=x.length;need(model?.normalizedClip===12&&model.means?.length===n&&model.scales?.length===n&&model.coefficients?.length===n+1&&model.means.every(Number.isFinite)&&model.scales.every(v=>Number.isFinite(v)&&v>0)&&model.coefficients.every(r=>r.length===outputs&&r.every(Number.isFinite)),'MODEL');const z=[1,...x.map((v,i)=>Math.max(-12,Math.min(12,(v-model.means[i])/model.scales[i])))];return Array.from({length:outputs},(_,side)=>z.reduce((v,a,i)=>v+a*model.coefficients[i][side],0));}
export function quoteOutcomeForecast(report,bank,x){
 need(report?.mode==='quote_fill_conditional_selector_research'&&JSON.stringify(report.features)===JSON.stringify(MICROPRICE_FEATURES)&&bank?.status==='research_selector_frozen'&&x?.length===MICROPRICE_FEATURES.length&&x.every(Number.isFinite),'BINDING');
 need(bank.policy?.feeTicks===0&&bank.policy.latencyMs===3000&&Number.isFinite(bank.selector?.minimumExpectedNetTicks)&&bank.selector.minimumExpectedNetTicks>=0&&Number.isFinite(bank.selector?.minimumAnyFillProbability)&&bank.selector.minimumAnyFillProbability>=0&&bank.selector.minimumAnyFillProbability<=1,'SELECTOR');
 const arrival=predict(bank.model?.arrival,x,2),anyFill=Math.max(0,Math.min(1,arrival[0])),paired=Math.max(0,Math.min(anyFill,arrival[1])),conditionalNetTicks=Math.max(-200,Math.min(199,predict(bank.model?.conditionalValue,x,1)[0]));
 const expectedNetTicks=anyFill*conditionalNetTicks;
 return{anyFillProbability:anyFill,pairedProbability:paired,conditionalNetTicks,expectedNetTicks,
  selected:expectedNetTicks>=bank.selector.minimumExpectedNetTicks&&anyFill>=bank.selector.minimumAnyFillProbability,
  executionAuthority:false,profitabilityEstablished:false};
}
