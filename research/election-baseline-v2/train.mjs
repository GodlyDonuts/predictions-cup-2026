import fs from 'node:fs';
import assert from 'node:assert/strict';
import { parseCsv, hash, senateRaces, presidentialTables, buildRows, fitRidge, predict, expandingCheck, metrics, probabilityFromMargin } from './model.mjs';
const base='research/election-baseline-v2', previous='research/election-baseline-v1';
const revision=process.argv.includes('--affiliation-invariant')?'affiliation-invariant':'initial';
const definitionBytes=fs.readFileSync(`${base}/definition.json`), definition=JSON.parse(definitionBytes);
assert(Date.parse(definition.declaredAt)<=Date.now());
const manifests=['PEJ5QU','42MVDX'].map(x=>JSON.parse(fs.readFileSync(`${previous}/sources/${x}-manifest.json`)));
for(const m of manifests)for(const f of m.files)assert.equal(hash(fs.readFileSync(`${previous}/sources/${f.filename}`)),f.sha256);
const overridesBytes=fs.readFileSync(`${base}/final-stage-overrides.json`);
const senate=senateRaces(parseCsv(fs.readFileSync(`${previous}/sources/1976-2024-senate-state.csv`,'utf8')),JSON.parse(overridesBytes),{allowUncompetitiveUnknown:revision==='affiliation-invariant'});
const president=presidentialTables(parseCsv(fs.readFileSync(`${previous}/sources/1976-2024-president.csv`,'utf8')));
const rows=buildRows(senate,president,definition.minimumTrainingYear);
const development=definition.ridgeGrid.map(lambda=>{
  const predictions=expandingCheck(rows,lambda,definition.developmentLastYear), summary=metrics(predictions);
  return {lambda,metrics:summary,cycleMse:summary.cycles.reduce((sum,y)=>sum+metrics(predictions.filter(r=>r.year===y)).rmse**2,0)/summary.cycles.length};
}).sort((a,b)=>a.cycleMse-b.cycleMse||a.lambda-b.lambda);
const selected=development[0], residualRms=Math.max(.10,selected.metrics.rmse), diagnostics=[];
for(const year of definition.observedDiagnosticYears){
  const model=fitRidge(rows.filter(r=>r.year<year),selected.lambda); assert(model.trainedThroughYear<year);
  for(const r of rows.filter(r=>r.year===year)){
    const predictedMargin=predict(model,r.x)+r.nationalBaseline, winProbability=probabilityFromMargin(predictedMargin,residualRms),outcome=Number(r.winnerRepublican);
    diagnostics.push({key:r.key,year,predictedMargin,actualMargin:r.actualMargin,residual:r.actualMargin-predictedMargin,trainedThroughYear:model.trainedThroughYear,clocks:r.clocks,outcome,winProbability});
  }
}
const model=fitRidge(rows,selected.lambda), at=new Date().toISOString();
const diagnosticSummary={...metrics(diagnostics),correctDirections:diagnostics.filter(r=>(r.predictedMargin>0)===(r.actualMargin>0)).length,
  brier:diagnostics.reduce((s,r)=>s+(r.winProbability-r.outcome)**2,0)/diagnostics.length,
  logLoss:diagnostics.reduce((s,r)=>s-(r.outcome*Math.log(r.winProbability)+(1-r.outcome)*Math.log(1-r.winProbability)),0)/diagnostics.length,
  byCycle:definition.observedDiagnosticYears.map(y=>metrics(diagnostics.filter(r=>r.year===y))),untouched:false,
  reason:'These cycles were already inspected in V1 and the runoff correction. Outcome diagnostics only; no historical market PnL or original forecast vintage.'};
const report={schemaVersion:2,id:definition.id,revision,at,definitionHash:hash(definitionBytes),overridesHash:hash(overridesBytes),sourceManifests:manifests,
  includedRaces:senate.races.size,rows:rows.length,exclusions:senate.exclusions,corrections:senate.corrections,development,
  selectedLambda:selected.lambda,selectedUsingYearsThrough:definition.developmentLastYear,residualRms,model,modelHash:hash(Buffer.from(JSON.stringify(model))),
  diagnostics:diagnosticSummary,diagnosticRows:diagnostics,qualifiedProductionForecasts:0,profitabilityEstablished:false,limitations:definition.limitations};
const reportPath=revision==='initial'?`${base}/model-report.json`:`${base}/model-report-${revision}.json`;
const summaryPath=revision==='initial'?`${base}/model-summary.json`:`${base}/model-summary-${revision}.json`;
if(process.argv.includes('--verify')){
  const saved=JSON.parse(fs.readFileSync(summaryPath));assert.equal(saved.modelHash,report.modelHash);assert.equal(saved.rows,report.rows);
  assert.deepEqual(saved.diagnostics,report.diagnostics);assert.equal(saved.overridesHash,report.overridesHash);assert.equal(saved.definitionHash,report.definitionHash);
  assert.equal(saved.fullReportHash,hash(fs.readFileSync(reportPath)));
  console.log(JSON.stringify({revision,modelFitReproduced:true,immutableReportHashVerified:true,rows:report.rows}));process.exit(0);
}
fs.writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n',{flag:'wx'});
const summary={...report};delete summary.diagnosticRows;delete summary.exclusions;
summary.excludedRaces=report.exclusions.length;summary.exclusionsByReason=Object.fromEntries([...new Set(report.exclusions.map(x=>x.reason))].map(reason=>[reason,report.exclusions.filter(x=>x.reason===reason).length]));
summary.fullReportHash=hash(fs.readFileSync(reportPath));summary.fullReportFilename=reportPath.split('/').at(-1);
fs.writeFileSync(summaryPath,JSON.stringify(summary,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({rows:report.rows,includedRaces:report.includedRaces,excludedRaces:summary.excludedRaces,selectedLambda:report.selectedLambda,diagnostics:diagnosticSummary,corrections:report.corrections,qualifiedProductionForecasts:0,profitabilityEstablished:false}));
