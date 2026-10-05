// Derived status only. Dispatch authority still lives in the controller fences.
export function productionEntryState({activation,nowMs,researchMarketIds=[],owned={},activeIntent=null,status,reason}) {
 const start=Date.parse(activation.startedAt),end=Date.parse(activation.entryExpiresAt);
 const clockValid=[start,end,nowMs].every(Number.isFinite)&&end>start;
 const phase=!clockValid?'invalid_clock':nowMs<start?'not_started':nowMs>=end?'closed':'open';
 const entryWindow={phase,startedAt:activation.startedAt,expiresAt:activation.entryExpiresAt,open:phase==='open'};
 const positions=Object.values(owned).filter(p=>p.quantity>0).length;
 const currentResearchMarkets=[...new Set(researchMarketIds)];
 const ordinary=['monitoring','waiting_for_signal','waiting_for_research','entry_window_closed','entry_window_not_started','managing_inventory'].includes(status);
 // Preserve failures and uncertain/open orders; window expiry never resolves them.
 if(ordinary&&!activeIntent){
  if(phase!=='open'){
   status=positions?'managing_inventory':phase==='closed'?'entry_window_closed':phase==='not_started'?'entry_window_not_started':'entry_clock_invalid';
   reason=phase==='closed'?'ENTRY_WINDOW_CLOSED':phase==='not_started'?'ENTRY_WINDOW_NOT_STARTED':'INVALID_ACTIVATION_CLOCK';
  }else if(!positions&&!currentResearchMarkets.length){status='waiting_for_research';reason='NO_CURRENT_RESEARCH';}
 }
 return {status,reason,entryWindow,currentResearchMarkets,attributedPositionsCount:positions,entryAuthorityExtended:false};
}
