export const dynamic='force-dynamic';
import {paperCohorts,paperWalletView} from '../../wallet-view.mjs';
export async function GET(){
 let execution;try{const r=await fetch('http://127.0.0.1:8787/api/execution',{cache:'no-store',signal:AbortSignal.timeout(4000)});if(!r.ok)throw Error();const x=await r.json();execution={status:x.status,reason:x.reason??null,updatedAt:x.updatedAt??null,ordersSubmitted:x.ordersSubmitted??0,attributedFills:x.fills?.length??0,realizedPnl:x.realizedPnl??null,limits:x.limits??null};}catch{execution={status:'unavailable'};}
 let official;try{const r=await fetch('http://127.0.0.1:8787/api/account',{cache:'no-store',signal:AbortSignal.timeout(4000)});if(!r.ok)throw Error();const d=await r.json();official={...d,id:'official',name:'Official account',official:true};}catch{official={id:'official',name:'Official account',official:true,available:false};}
 const wallets=await Promise.all(paperCohorts.map(async([id,name])=>{
  try{
   const response=await fetch(`http://127.0.0.1:8787/api/${id}`,{cache:'no-store',signal:AbortSignal.timeout(4000)});
   if(!response.ok)throw Error('Unavailable');const d=await response.json();if(!d.strategy)throw Error('Unavailable');
   return paperWalletView(d,id,name);
  }catch{return {id,name,available:false,status:'unavailable'};}
 }));
 return Response.json({updatedAt:new Date().toISOString(),execution,wallets:[official,...wallets]},{headers:{'Cache-Control':'no-store'}});
}
