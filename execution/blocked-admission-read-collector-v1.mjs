// Called only by the existing serialized account client. Owns no credentials,
// fetch, timers, write permission, STOP mutation or replacement policy.
export async function collectBlockedAdmissionAccountV1({api,receiptOf,scope,intent,canRead,now=Date.now}){
 const need=(v,c)=>{if(!v)throw Error('BLOCKED_READ_'+c);};need(api&&typeof api.dispatch==='function'&&typeof receiptOf==='function'&&typeof canRead==='function','DEPENDENCIES');
 need(intent.orderId===null&&intent.actualFetchCount===0&&Date.parse(intent.body.expirationDate)<now(),'EXPIRED_UNFETCHED');
 const T=scope.tournamentId,ex=intent.body.exchangeId,endpoints=['/tournaments/'+scope.tournamentSlug,'/tournaments/'+scope.tournamentSlug+'/portfolio/positions','/orders?status=open&limit=200&tournamentId='+T,'/orders?status=all&exchangeId='+ex+'&limit=200&tournamentId='+T],receipts=[];
 for(const endpoint of endpoints){need(canRead()===true,'OWNER_OR_STOP_PROVENANCE');const guard=()=>{need(canRead()===true,'OWNER_OR_STOP_PROVENANCE');return true;},response=await api.dispatch({method:'GET',endpoint,beforeDispatch:guard,beforeNetwork:guard});receipts.push(receiptOf(response,{method:'GET',requestBodyHash:null}));}
 for(const r of receipts.slice(2)){const d=JSON.parse(r.bodyText);need(d.pagination?.hasMore===false&&d.pagination.nextCursor===null,'COMPLETE_SINGLE_PAGE');}
 return{cash:receipts[0],positions:receipts[1],openOrdersPages:[receipts[2]],scopedOrdersPages:[receipts[3]]};
}

