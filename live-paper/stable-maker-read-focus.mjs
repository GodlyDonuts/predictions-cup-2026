// Retain every existing risk slot. Spend only remaining discovery slots on a
// fixed ten-minute group so a five-minute stability feature can actually form.
export function focusStableMakerReads(plan,focus,universe,{now=Date.now(),protectedMarketIds=[],excludedMarketIds=[]}={}){
 if(!focus||focus.failed||!focus.candidateMarketIds?.length)return plan;
 const scope=new Map(universe.exchanges.map(x=>[x.marketId,x])),protectedIds=new Set(protectedMarketIds),excluded=new Set(excludedMarketIds);
 const available=focus.candidateMarketIds.filter(id=>scope.has(id)&&!protectedIds.has(id)&&!excluded.has(id));
 const slots=plan.reads.map((r,i)=>protectedIds.has(r.marketId)?null:i).filter(i=>i!==null);
 if(!slots.length||!available.length)return plan;
 const cutoff=Date.parse(focus.cutoff);if(!Number.isFinite(cutoff)||!Number.isFinite(now)||now<cutoff)throw Error('STABLE_READ_CLOCK');
 const offset=Math.floor((now-cutoff)/600000)*slots.length%available.length,selected=Array.from({length:Math.min(slots.length,available.length)},(_,i)=>available[(offset+i)%available.length]);
 const reads=plan.reads.map(x=>({...x}));for(let i=0;i<selected.length;i++){const spec=scope.get(selected[i]);reads[slots[i]]={marketId:spec.marketId,exchangeId:spec.exchangeId,option:'YES',path:'supplementalDepth',reason:'stable_paper_history'};}
 // Avoid retaining a discovery duplicate when the fixed group replaced a slot.
 const seen=new Set();for(let i=0;i<reads.length;i++){if(seen.has(reads[i].marketId)){const original=plan.reads.find(r=>!seen.has(r.marketId)&&!selected.includes(r.marketId));if(!original)throw Error('STABLE_READ_DUPLICATE');reads[i]={...original};}seen.add(reads[i].marketId);}
 return {...plan,reads,entryMarketIds:selected,stableMakerReadFocus:{groupMs:600000,selectedMarketIds:selected,protectedMarketIds:[...protectedIds],readsAdded:0,executionAuthority:false}};
}
