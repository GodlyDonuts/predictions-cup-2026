import {payloadHash} from '../execution/ledger.mjs';

// Both funded consumers must reach the same event before either advances it.
// Sample the actual wall clock once, after both are ready; never backdate fills.
export function pairedClock(now=()=>new Date().toISOString()) {
  let pending=null, failure=null;
  const abort=error=>{failure=error;for(const waiter of pending?.waiters??[])waiter.reject(error);pending=null;};
  const clock=participant=>(event,index)=>new Promise((resolve,reject)=>{
    if(failure){reject(failure);return;}
    if(![0,1].includes(participant)||!Number.isSafeInteger(index)||index<1){const e=Error('PAIRED_CLOCK_PARTICIPANT');abort(e);reject(e);return;}
    const hash=payloadHash(event);
    if(!pending)pending={index,hash,waiters:[]};
    if(pending.index!==index||pending.hash!==hash||pending.waiters.some(w=>w.participant===participant)){
      const e=Error('PAIRED_CLOCK_EVENT_MISMATCH');abort(e);reject(e);return;
    }
    pending.waiters.push({participant,resolve,reject});
    if(pending.waiters.length===2){
      const at=now();
      if(!Number.isFinite(Date.parse(at))||Date.parse(at)<Date.parse(event.receivedAt)){
        abort(Error('PAIRED_CLOCK_CAUSALITY'));return;
      }
      const waiters=pending.waiters;pending=null;for(const w of waiters)w.resolve(at);
    }
  });
  return{clock,abort};
}
