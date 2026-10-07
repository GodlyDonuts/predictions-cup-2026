import fs from 'node:fs';import path from 'node:path';
export function paperConsumerStopped(runtimeDir,filesystem=fs){
 return ['EXECUTION_STOP','COLLECTOR_STOP','STOP'].some(name=>filesystem.existsSync(path.join(runtimeDir,name)));
}
export function frozenPaperStopView(previous){
 return{...previous,status:'paused',reason:'OPERATOR_STOP',valuationFrozen:true,accountingRetained:true,executionAuthority:false};
}

