// collect() mutates only counters and the selected exchange. Preserve other
// immutable market objects instead of deep-cloning all 237 dedup dictionaries.
export function tapeMarketCopyV1(state,exchangeId){
 if(!state||typeof state.markets!=='object'||state.markets===null||Array.isArray(state.markets)||typeof exchangeId!=='string'||!/^\d+$/.test(exchangeId))throw Error('TAPE_COPY_SCOPE');
 const next={...state,markets:{...state.markets}};
 if(state.markets[exchangeId])next.markets[exchangeId]=structuredClone(state.markets[exchangeId]);
 return next;
}
