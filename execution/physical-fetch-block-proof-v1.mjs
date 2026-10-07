import {payloadHash} from './core.mjs';
export function physicalFetchBlockProofV1({spec,code,at}){
 if(spec?.method!=='POST'||spec.endpoint!=='/orders'||typeof spec.bodyJson!=='string'||code!=='BAAL_SCALED_PHYSICAL_FETCH_FENCE'||!Number.isFinite(Date.parse(at)))throw Error('PHYSICAL_FETCH_PROOF_INPUT');
 return{schemaVersion:1,kind:'affirmative_before_actual_fetch_clock_block_v1',method:'POST',endpoint:'/orders',requestBodyHash:payloadHash(JSON.parse(spec.bodyJson)),at,physicalFetchReached:false,code};
}
