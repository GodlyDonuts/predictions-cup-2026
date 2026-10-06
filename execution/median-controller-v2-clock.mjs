// Operational timestamp adapter only. The frozen V2 bank, economic policy,
// inherited order/ownership guards and both network fences remain unchanged.
import {MedianCompetitionControllerV2} from './median-controller-v2.mjs';

export class MedianCompetitionControllerV2Clock extends MedianCompetitionControllerV2 {
 createProposal(...args){
  // The inherited method is synchronous. Freeze just its single proposal's
  // creation clock; never leave a frozen clock across an await or dispatch.
  const originalNow=this.now,at=originalNow.call(this);
  this.now=()=>at;
  try{return super.createProposal(...args);}finally{this.now=originalNow;}
 }
}
