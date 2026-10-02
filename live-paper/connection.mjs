// Startup GET recovery shares the existing client, limiter and persisted admission.
export async function connectTournament(api,c,{stopped,onRetry,pause,now=Date.now}){
 let attempts=0;
 while(!stopped()){
  try{
   const data=(await api.request('/tournaments/'+c.tournamentSlug)).data;
   if(!data||data.id!==c.tournamentId||data.currencyName!=='SUSQies'||data.status!=='active'||data.isPendingEnrolment)throw Error('Tournament identity/status mismatch');
   return data;
  }catch(e){
   if(stopped())return null;
   if(!e.recoverable||api.manualReview)throw e;
   const delay=Math.max(Math.min(60000,10000*2**Math.min(attempts,3)),Math.max(0,api.cooldown-now()));
   attempts++;await onRetry({attempts,delay,reason:e.status?'http_'+e.status:'connection_unavailable'});await pause(delay);
  }
 }
 return null;
}
