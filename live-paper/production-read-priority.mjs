// Preserve the frozen membership and read budget while serving official risk
// and eligible production candidates before slower counterfactual observers.
export function prioritizeProductionReads(plan,{candidateMarketIds=[],ownedMarketIds=[]}={}){
 const owned=new Set(ownedMarketIds),candidate=new Set(candidateMarketIds);
 const rank=id=>owned.has(id)?0:candidate.has(id)?1:2;
 return {...plan,reads:plan.reads.map((read,index)=>({read,index})).sort((a,b)=>rank(a.read.marketId)-rank(b.read.marketId)||a.index-b.index).map(x=>x.read)};
}
