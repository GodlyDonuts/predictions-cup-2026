import {MICROPRICE_FEATURES} from './realtime-microprice-dataset.mjs';
// Model output is a research forecast, never an execution permission.
export function micropriceForecast(bank,x){
 const m=bank?.model,n=MICROPRICE_FEATURES.length;
 if(bank?.mode!=='realtime_microprice_forecast_research'||JSON.stringify(bank.features)!==JSON.stringify(MICROPRICE_FEATURES)||!Array.isArray(x)||x.length!==n||!x.every(Number.isFinite)||m?.normalizedClip!==12||!Array.isArray(m.means)||m.means.length!==n||!m.means.every(Number.isFinite)||!Array.isArray(m.scales)||m.scales.length!==n||!m.scales.every(v=>Number.isFinite(v)&&v>0)||!Array.isArray(m.coefficients)||m.coefficients.length!==n+1||!m.coefficients.every(r=>r.length===2&&r.every(Number.isFinite))||!Array.isArray(bank.lowerForecastErrorTicks)||bank.lowerForecastErrorTicks.length!==2||!bank.lowerForecastErrorTicks.every(v=>Number.isFinite(v)&&v>=0))throw Error('MICROPRICE_FORECAST_BINDING');
 const z=[1,...x.map((v,i)=>Math.max(-12,Math.min(12,(v-m.means[i])/m.scales[i])))],predictions=[0,1].map(side=>z.reduce((v,a,i)=>v+a*m.coefficients[i][side],0));
 return{bidChangeTicks:predictions,lowerBidChangeTicks:predictions.map((v,i)=>v-bank.lowerForecastErrorTicks[i]),executionAuthority:false,profitabilityEstablished:false};
}
