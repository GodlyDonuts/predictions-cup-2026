import {payloadHash} from '../execution/ledger.mjs';
const need=(v,c)=>{if(!v)throw Error('TAPE_DELTA_'+c);};
const header=state=>{const {markets,...h}=state;need(h.schemaVersion===1&&h.seenIdentityVersion===2&&typeof h.scopeHash==='string'&&h.scopeHash.length===64,'HEADER');for(const k of ['pages','tradesCaptured','duplicates'])need(Number.isSafeInteger(h[k])&&h[k]>=0,'COUNTER');need(h.lastAt===null||Number.isFinite(Date.parse(h.lastAt)),'CLOCK');return h;};
const root=(h,hashes)=>payloadHash({header:h,marketHashes:hashes});
// The producer owns the seed and must retain copy-on-write immutability.
// prepare() has no durable effect; advance only after the outer journal commits.
export class TapeCheckpointDeltaV1{
 constructor(state,sequence){need(Number.isSafeInteger(sequence)&&sequence>=0&&state.markets&&typeof state.markets==='object'&&!Array.isArray(state.markets),'SEED');this.state=state;this.sequence=sequence;this.hashes=Object.fromEntries(Object.entries(state.markets).map(([id,m])=>[id,payloadHash(m)]));this.root=root(header(state),this.hashes);}
 prepare(next,exchangeId){
  need(typeof exchangeId==='string'&&/^\d+$/.test(exchangeId)&&next.markets[exchangeId],'MARKET');const h=header(next),old=header(this.state);
  need(h.scopeHash===old.scopeHash&&h.pages===old.pages+1&&h.tradesCaptured>=old.tradesCaptured&&h.duplicates>=old.duplicates,'TRANSITION');
  need(old.lastAt===null||Date.parse(h.lastAt)>=Date.parse(old.lastAt),'CLOCK');
  for(const [id,m]of Object.entries(this.state.markets))if(id!==exchangeId)need(next.markets[id]===m,'UNRELATED_MARKET_MUTATION');
  for(const id of Object.keys(next.markets))need(id===exchangeId||Object.hasOwn(this.state.markets,id),'UNRELATED_MARKET_INSERTION');
  const marketHash=payloadHash(next.markets[exchangeId]),hashes={...this.hashes,[exchangeId]:marketHash};
  return{codec:'tape_market_delta_v1',sequence:this.sequence+1,baseRoot:this.root,root:root(h,hashes),header:h,exchangeId,marketHash,market:next.markets[exchangeId]};
 }
 accept(next,frame){const expected=this.prepare(next,frame.exchangeId);need(payloadHash(expected)===payloadHash(frame),'PREPARED_FRAME_CHANGED');this.state=next;this.sequence=frame.sequence;this.hashes={...this.hashes,[frame.exchangeId]:frame.marketHash};this.root=frame.root;}
 apply(frame){
  need(frame?.codec==='tape_market_delta_v1'&&frame.sequence===this.sequence+1&&frame.baseRoot===this.root,'SEQUENCE_OR_BASE');need(typeof frame.exchangeId==='string'&&/^\d+$/.test(frame.exchangeId)&&payloadHash(frame.market)===frame.marketHash,'MARKET_HASH');
  const next={...frame.header,markets:{...this.state.markets,[frame.exchangeId]:structuredClone(frame.market)}},expected=this.prepare(next,frame.exchangeId);
  need(expected.root===frame.root&&payloadHash(expected.header)===payloadHash(frame.header),'ROOT_HASH');this.accept(next,expected);return this.state;
 }
}
