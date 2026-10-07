export function knownCancelPhysicalDeadlineV1({meta,intent,evidence,now}){
 const need=(v,c)=>{if(!v)throw Error('CANCEL_PHYSICAL_'+c);};need(meta?.method==='DELETE'&&intent?.orderId&&intent.cancelPending===true&&meta.endpoint==='/orders/'+intent.orderId&&meta.requestBodyHash===null,'OWN_BOUND_DELETE');
 const d=JSON.parse(evidence.order.bodyText);need(d.id===Number(intent.orderId)&&d.exchangeId===intent.exchangeId&&d.side===intent.body.side&&d.action===intent.body.action&&d.open===true&&d.expirationDate===intent.body.expirationDate&&now>=Date.parse(intent.body.expirationDate),'EXPIRED_CURRENT_OWN_ORDER');
 const deadline=Math.min(Date.parse(evidence.order.requestStartedAt)+20000,Date.parse(evidence.globalRisk.at)+20000);need(Number.isSafeInteger(deadline)&&now<deadline,'FRESH_ORDER_AND_RISK');return{notAfter:deadline};
}
