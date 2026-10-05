import fs from 'node:fs';
import path from 'node:path';

// Retirement is distinct from a temporary STOP. Ordinary stopped experiments
// retain inventory monitoring; only the explicit operator retirement removes
// counterfactual wallets from collection priorities. Official risk is separate.
export function paperExperimentRetired(runtimeDir, marker, fileSystem=fs) {
 if(!['DIP_STOP','SHOCK_STOP','ENTRY_COMPARISON_STOP','NATIVE_PRICE_PAPER_STOP'].includes(marker))throw Error('INVALID_RETIREMENT_SCOPE');
 try {
  const file=path.join(runtimeDir,marker),stat=fileSystem.statSync(file);
  if(!stat.isFile()||stat.size>4096)return false;
  const record=JSON.parse(fileSystem.readFileSync(file,'utf8'));
  return record.owner==='Codex'&&record.reason==='Retired obsolete experiment at human request'&&record.restartRequires==='Explicitly selected future experiment'&&typeof record.at==='string'&&Number.isFinite(Date.parse(record.at));
 }catch{return false;}
}
