// Wait before the first fresh receipt; never consume requests to obtain capacity.
export async function waitKnownOrderReadCapacityV1({orderId,hasReadHeadroom,canRead,now=Date.now,pause=ms=>new Promise(r=>setTimeout(r,ms))}){
 if(typeof orderId!=='string'||!(/^[1-9]\d*$/.test(orderId))||!Number.isSafeInteger(Number(orderId)))throw Error('KNOWN_CAPACITY_ORDER');const start=now();let waits=0;
 while(true){if(canRead()!==true)throw Error('KNOWN_CAPACITY_STOPPED');const ready=hasReadHeadroom(11);if(ready===true)return{waits,waitedMs:now()-start,readsReserved:11,networkRequests:0};if(ready!==false)throw Error('KNOWN_CAPACITY_CALLBACK');if(now()-start>=60000)throw Error('KNOWN_CAPACITY_TIMEOUT');await pause(250);waits++;}
}
