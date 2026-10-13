// Observe only normalized trade_observed records AFTER the existing verifier
// and durable capture sink. Activity ranking never supplies trading authority.
import {instantNs} from '../feed/contract.mjs';import {payloadHash} from '../execution/ledger.mjs';
export function createMakerRollingFlowV1({scope,now=Date.now,windowMs=900000,maximumRecords=50000,maximumIdleMs=120000}){
  const need=(v,c)=>{if(!v)throw Error('MAKER_ROLLING_FLOW_'+c);};
  need(typeof scope?.tournamentId==='string'&&Array.isArray(scope.markets)&&typeof now==='function','SCOPE');
  need(Number.isSafeInteger(windowMs)&&windowMs>0&&windowMs<=900000&&Number.isSafeInteger(maximumRecords)&&maximumRecords>0&&
    maximumRecords<=100000&&Number.isSafeInteger(maximumIdleMs)&&maximumIdleMs>0&&maximumIdleMs<=windowMs,'POLICY');
  const members=new Map();for(const m of scope.markets){need(typeof m.marketId==='string'&&typeof m.exchangeId==='string'&&
    /^[1-9]\d*$/.test(m.marketId)&&/^[1-9]\d*$/.test(m.exchangeId)&&!members.has(m.marketId),'MEMBER');members.set(m.marketId,m.exchangeId);}
  let lastNow=0,lastPruneAt=-Infinity,duplicates=0,rejected=0,overflow=0;const records=new Map();
  function clock(){const n=now();need(Number.isSafeInteger(n)&&n>=lastNow,'CLOCK');lastNow=n;return n;}
  function prune(at,force=false){if(!force&&at-lastPruneAt<1000)return;
    for(const [key,r]of records)if(r.executedMs<at-windowMs)records.delete(key);lastPruneAt=at;}
  function observe(record){
    if(record?.kind!=='trade_observed')return false;const at=clock();prune(at);const t=record.trade;
    need(t?.tournamentId===scope.tournamentId&&members.get(t.marketId)===t.exchangeId,'FOREIGN_SCOPE');
    need(typeof t.id==='string'&&t.id.length>0&&t.id.length<=256&&Number.isSafeInteger(t.sequence)&&t.sequence>=0&&
      Number.isSafeInteger(t.quantity)&&t.quantity>0&&t.quantity<=2147483647&&Number.isFinite(t.price)&&t.price>=0&&t.price<=1,'TRADE');
    const executedNs=instantNs(t.executedAt),observedNs=instantNs(record.observedAt),nowNs=BigInt(at)*1000000n;
    if(observedNs>nowNs||executedNs>nowNs+1000000000n||executedNs<nowNs-BigInt(windowMs)*1000000n||
      observedNs-executedNs>5000000000n||executedNs-observedNs>1000000000n){rejected++;return false;}
    const key=payloadHash([scope.tournamentId,t.marketId,t.exchangeId,t.id]);
    const fingerprint=payloadHash([t.sequence,executedNs.toString(),t.quantity,t.price]),prior=records.get(key);
    if(prior){need(prior.fingerprint===fingerprint,'MATCH_ID_CONFLICT');duplicates++;return false;}
    if(records.size>=maximumRecords){overflow++;return false;}
    records.set(key,{marketId:t.marketId,exchangeId:t.exchangeId,quantity:t.quantity,executedMs:Number(executedNs/1000000n),
      observedMs:Number(observedNs/1000000n),fingerprint});return true;
  }
  function snapshot(){const at=clock();prune(at,true);const markets=new Map();
    for(const r of records.values()){let m=markets.get(r.marketId);if(!m){m={marketId:r.marketId,exchangeId:r.exchangeId,trades:0,shares:0,
      lastExecutedMs:r.executedMs,lastObservedMs:r.observedMs};markets.set(r.marketId,m);}m.trades++;m.shares+=r.quantity;
      m.lastExecutedMs=Math.max(m.lastExecutedMs,r.executedMs);m.lastObservedMs=Math.max(m.lastObservedMs,r.observedMs);}
    const ranked=[...markets.values()].sort((a,b)=>b.shares-a.shares||b.trades-a.trades||a.marketId.localeCompare(b.marketId));
    return{at:new Date(at).toISOString(),ranked,records:records.size,duplicates,rejected,overflow,
      sampleTruncated:overflow>0,networkRequests:0,executionAuthority:false,profitabilityEstablished:false};
  }
  function recoveryPriority(limit=3){need(Number.isSafeInteger(limit)&&limit>=0&&limit<=4,'LIMIT');const s=snapshot(),at=Date.parse(s.at);
    const selected=s.ranked.filter(m=>at-m.lastObservedMs<=maximumIdleMs).slice(0,limit);
    // Re-reading a rank must not renew an old source observation's TTL. Use the
    // oldest selected observation so every market satisfies the host age gate.
    return{at:selected.length?new Date(Math.min(...selected.map(m=>m.lastObservedMs))).toISOString():s.at,
      generatedAt:s.at,marketIds:selected.map(m=>m.marketId)};}
  return Object.freeze({observe,snapshot,recoveryPriority});
}
