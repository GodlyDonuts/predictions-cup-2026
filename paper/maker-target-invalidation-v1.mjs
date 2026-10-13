// Market-local receipt failures must not erase unrelated desired quotes.
// This changes no cash, fill, order acceptance, or retirement evidence.
export function invalidateMakerTargetsV1(targets, latest, row) {
 if(!['invalidation','session_stop'].includes(row.kind))throw Error('MAKER_TARGET_INVALIDATION_KIND');
 const global=row.kind==='session_stop'||row.payload.marketId===null;
 const marketId=global?null:row.payload.marketId;
 if(!global&&(typeof marketId!=='string'||!/^[1-9]\d*$/.test(marketId)))throw Error('MAKER_TARGET_INVALIDATION_SCOPE');
 if(global){latest.clear();return {targets:{},global:true,marketId:null};}
 const next={};for(const [exchange,quotes]of Object.entries(targets)){
  const surviving=quotes.filter(q=>q.marketId!==marketId);
  if(surviving.length)next[exchange]=surviving;
 }
 for(const [exchange,record]of latest)if(record.payload.book.marketId===marketId)latest.delete(exchange);
 return {targets:next,global:false,marketId};
}
