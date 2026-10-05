// An operator drain leaves the frozen bank, original quotes, tape and inventory
// intact. A recorded boundary clears entry features before an unowned book;
// owned books retain the original paired-risk reduction and exit policy.
export function wrapCoupledDrain(host,{isDraining}){
 return {...host,status(){const status=host.status();return isDraining()?{...status,operatorMode:'drain_existing_inventory_and_quotes'}:status;},focus(){const focus=host.focus();return isDraining()?{...focus,candidateMarketIds:[],operatorMode:'drain_existing_inventory_and_quotes'}:focus;},
  onBook(publication,marketId,accountEligibility){if(isDraining()){
   const owned=Object.values(host.status().wallets).some(w=>w.positions.some(p=>p.marketId===marketId&&p.quantity>0));
   if(!owned)host.boundary('operator_drain_unowned_entry_features');
  }return host.onBook(publication,marketId,accountEligibility);}
 };
}
