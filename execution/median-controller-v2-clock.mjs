// Operational timestamp adapter only. The frozen V2 bank, economic policy,
// inherited order/ownership guards and both network fences remain unchanged.
import {MedianCompetitionControllerV2} from './median-controller-v2.mjs';
import fs from 'node:fs';
import path from 'node:path';

export class MedianCompetitionControllerV2Clock extends MedianCompetitionControllerV2 {
 entryWindowAdmitted(now){
  if(this.runtimeDir&&fs.existsSync(path.join(this.runtimeDir,'MEDIAN_ENTRY_DRAIN'))){this.reason='MEDIAN_ENTRY_DRAIN';return false;}
  return super.entryWindowAdmitted(now);
 }
 createProposal(...args){
  // The inherited method is synchronous. Freeze just its single proposal's
  // creation clock; never leave a frozen clock across an await or dispatch.
  const originalNow=this.now,at=originalNow.call(this);
  this.now=()=>at;
  try{return super.createProposal(...args);}finally{this.now=originalNow;}
 }
}
