export function createMakerFastCadenceV1({now=Date.now,maximumQuietMs=5000}={}){
 if(!Number.isSafeInteger(maximumQuietMs)||maximumQuietMs<1000)throw Error('MAKER_CADENCE_POLICY');let signature=null,lastUpdate=null,lastFocus=null;
 return{needsUpdate(next){return signature!==next||lastUpdate===null||now()-lastUpdate>=maximumQuietMs;},updated(next){signature=next;lastUpdate=now();},needsFocus(){return lastFocus===null||now()-lastFocus>=30000;},focused(){lastFocus=now();}};
}
