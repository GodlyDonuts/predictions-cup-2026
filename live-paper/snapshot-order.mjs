import {scopedSnapshots} from './depth-scope.mjs';
export const LEGACY_SNAPSHOT_ORDER='core_then_supplemental_v1';
export const RECEIPT_SNAPSHOT_ORDER='actual_receipt_order_v1';
export function validateSnapshotOrder(value=LEGACY_SNAPSHOT_ORDER){
 if(![LEGACY_SNAPSHOT_ORDER,RECEIPT_SNAPSHOT_ORDER].includes(value))throw Error('Unknown snapshot order policy');return value;
}
export function orderedSnapshots(event,config,policy=LEGACY_SNAPSHOT_ORDER,fallbackAt=null){
 validateSnapshotOrder(policy);const rows=scopedSnapshots(event,config);
 if(policy===LEGACY_SNAPSHOT_ORDER)return rows;
 return rows.map((snapshot,index)=>{
  const at=Date.parse(snapshot.skipped?snapshot.at||event.clockAt||fallbackAt:snapshot.receivedAt);
  if(!Number.isFinite(at))throw Error('Invalid snapshot receipt clock');
  return {snapshot,index,at};
 }).sort((a,b)=>a.at-b.at||a.index-b.index).map(x=>x.snapshot);
}
