// Partial pooling changes the location, never manufactures narrower residual
// uncertainty from several correlated polls of the same election.
export function statePollBiasV1({model,races,state,priorRaceEquivalents=5}){
 if(!Number.isFinite(model.biasPoints)||!Number.isFinite(model.standardDeviationPoints)||model.standardDeviationPoints<=0||!Number.isSafeInteger(model.trainedThroughYear)||!Array.isArray(races)||typeof state!=='string'||!state||!Number.isFinite(priorRaceEquivalents)||priorRaceEquivalents<=0)throw Error('STATE_BIAS_INPUT');
 const seen=new Set();for(const r of races){if(typeof r.key!=='string'||seen.has(r.key)||!Number.isSafeInteger(r.year)||r.year>model.trainedThroughYear||typeof r.state!=='string'||!Number.isFinite(r.error))throw Error('STATE_BIAS_RACE');seen.add(r.key);}
 const local=races.filter(r=>r.state===state),mean=local.length?local.reduce((n,r)=>n+r.error,0)/local.length:model.biasPoints,weight=local.length/(local.length+priorRaceEquivalents),bias=model.biasPoints+weight*(mean-model.biasPoints);
 return{...model,family:'state_partial_pooling_location_v1',biasPoints:bias,state,localRaces:local.length,localCycles:[...new Set(local.map(r=>r.year))],unpooledStateBiasPoints:mean,globalBiasPoints:model.biasPoints,priorRaceEquivalents,localWeight:weight,varianceShrunk:false,priorStrengthIsJudgment:true,currentTransportValidated:false};
}
