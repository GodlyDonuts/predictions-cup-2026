// Replace an existing recovery slot, never add a client or a second GET slot.
import {createRealtimeHost} from './realtime-host.mjs';import {instant} from '../paper/fixed.mjs';
export function createRealtimePriorityHostV2({getRecoveryPriority=()=>null,maximumPriorityAgeMs=120000,...options}){
  if(typeof getRecoveryPriority!=='function'||!Number.isSafeInteger(maximumPriorityAgeMs)||maximumPriorityAgeMs<1||maximumPriorityAgeMs>900000)throw Error('REALTIME_PRIORITY_POLICY');
  const base=createRealtimeHost(options),members=new Set(options.scope.markets.map(m=>m.marketId)),now=options.now??Date.now;
  let busy=false,closed=false,streak=0,cursor=0,attempts=0,successes=0,backgroundCalls=0,staleReports=0,lastCode=null;
  const background=async()=>{streak=0;backgroundCalls++;return base.service();};
  async function service(){
    if(closed||busy)return false;busy=true;
    try{
      if(base.status().transport.state!=='subscribed'||streak>=3)return await background();
      const priority=getRecoveryPriority();if(priority===null)return await background();
      if(!Array.isArray(priority.marketIds)||priority.marketIds.length>4||new Set(priority.marketIds).size!==priority.marketIds.length||
        priority.marketIds.some(id=>!members.has(id)))throw Error('REALTIME_PRIORITY_SCOPE');
      const age=now()-instant(priority.at);if(age<0||age>maximumPriorityAgeMs){staleReports++;return await background();}
      for(let i=0;i<priority.marketIds.length;i++){
        const index=(cursor+i)%priority.marketIds.length,id=priority.marketIds[index],view=base.view(id);
        if(!view||view.ready||!view.needsSnapshot||view.reasons.includes('SETTLEMENT'))continue;
        const ticket=base.beginSharedSnapshot(id);if(!ticket)continue;
        cursor=(index+1)%priority.marketIds.length;streak++;attempts++;
        try{const response=await base.readSharedSnapshot(ticket);if(base.completeSharedSnapshot(ticket,response)){successes++;lastCode=null;}else lastCode='PRIORITY_RECOVERY_REJECTED';}
        catch{base.failSharedSnapshot(ticket);lastCode='PRIORITY_RECOVERY_UNAVAILABLE';}
        return true; // This replaces base.service() for this call; no second GET.
      }
      return await background();
    }finally{busy=false;}
  }
  function status(){return {...base.status(),priorityRecovery:{attempts,successes,backgroundCalls,staleReports,priorityStreak:streak,
    maximumPriorityBurst:3,maximumPriorityMarkets:4,maximumPriorityAgeMs,lastCode,additionalApiClients:0,additionalRecoverySlots:0}};}
  async function stop(){closed=true;await base.stop();}
  return Object.freeze({...base,service,status,stop});
}
