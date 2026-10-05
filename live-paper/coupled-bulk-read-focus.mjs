// Bulk history is already warm across the universe. Only fresh depth slots rotate.
export function focusCoupledBulkReads(plan,focus,universe,{now=Date.now(),protectedMarketIds=[],excludedMarketIds=[]}={}){
 if(!focus||focus.failed||!focus.candidateMarketIds?.length)return plan;
 const scope=new Map(universe.exchanges.map(x=>[x.marketId,x])),protectedIds=new Set(protectedMarketIds),excluded=new Set(excludedMarketIds),seen=new Set();
 const available=focus.candidateMarketIds.filter(id=>scope.has(id)&&!protectedIds.has(id)&&!excluded.has(id)&&!seen.has(id)&&seen.add(id));
 const slots=plan.reads.map((r,i)=>protectedIds.has(r.marketId)?null:i).filter(i=>i!==null);if(!slots.length||!available.length)return plan;
 const cutoff=Date.parse(focus.cutoff);if(!Number.isFinite(cutoff)||!Number.isFinite(now)||now<cutoff)throw Error('COUPLED_BULK_READ_CLOCK');
 const offset=Math.floor((now-cutoff)/30000)*slots.length%available.length,selected=Array.from({length:Math.min(slots.length,available.length)},(_,i)=>available[(offset+i)%available.length]);
 const reads=plan.reads.map(x=>({...x}));for(let i=0;i<selected.length;i++){const spec=scope.get(selected[i]);reads[slots[i]]={marketId:spec.marketId,exchangeId:spec.exchangeId,option:'YES',path:'supplementalDepth',reason:'bulk_stability_fresh_depth'};}
 const used=new Set();for(let i=0;i<reads.length;i++){if(used.has(reads[i].marketId)){const original=plan.reads.find(r=>!used.has(r.marketId)&&!selected.includes(r.marketId));if(!original)throw Error('COUPLED_BULK_READ_DUPLICATE');reads[i]={...original};}used.add(reads[i].marketId);}
 return{...plan,reads,entryMarketIds:selected,coupledBulkReadFocus:{rotationMs:30000,selectedMarketIds:selected,protectedMarketIds:[...protectedIds],readsAdded:0,executionAuthority:false}};
}
