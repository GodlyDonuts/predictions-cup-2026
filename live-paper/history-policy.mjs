export const LEGACY='global-reset-v1',ISOLATED='market-isolated-v2';
export function historyPolicy(value=LEGACY){if(![LEGACY,ISOLATED].includes(value))throw Error('Unknown ML history policy');return value;}
export function resetHistory(histories,marketId,policy=LEGACY){
 historyPolicy(policy);
 for(const [instrument,rows]of Object.entries(histories)){
  const row=rows.at(-1),market=row?.marketId??row?.book?.marketId;
  if(policy===LEGACY||marketId===null||market===marketId)delete histories[instrument];
 }
}
