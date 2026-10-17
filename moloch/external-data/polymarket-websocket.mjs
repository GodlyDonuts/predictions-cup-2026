// Public raw market stream. Primary schema/heartbeat checked 2026-10-17:
// https://docs.polymarket.com/market-data/realtime-data (Market Stream / API).
// No sequence field is documented: observed reconstruction is NOT a complete
// tape or verified sequence-continuity claim. Every reconnect needs a snapshot.
import {createHash} from 'node:crypto';
import WebSocket from 'ws';
export const MARKET_WS_URL='wss://ws-subscriptions-clob.polymarket.com/ws/market';
const SCALE=10n**18n;
const need=(v,c)=>{if(!v)throw Error(`POLY_WS_${c}`);};
const sha=s=>createHash('sha256').update(s).digest('hex');
const clock=n=>Number.isSafeInteger(n)&&n>=0;
const asset=s=>typeof s==='string'&&/^[0-9]{1,100}$/.test(s);
const condition=s=>typeof s==='string'&&/^0x[0-9a-fA-F]{64}$/.test(s);
function decimal(s,price=false){need(typeof s==='string'&&s.length<=80&&/^\d+(?:\.\d{1,18})?$/.test(s),'DECIMAL');const[a,b='']=s.split('.');const n=BigInt(a)*SCALE+BigInt(b.padEnd(18,'0'));need(!price||n<=SCALE,'PRICE');return n;}
function text(n){const a=n/SCALE,b=String(n%SCALE).padStart(18,'0').replace(/0+$/,'');return `${a}${b?'.'+b:''}`;}
function levels(rows,max){need(Array.isArray(rows)&&rows.length<=max,'LEVEL_BOUND');const m=new Map();for(const l of rows){need(l&&typeof l==='object','LEVEL');const p=decimal(l.price,true),q=decimal(l.size);need(!m.has(p),'DUPLICATE_LEVEL');if(q>0n)m.set(p,q);}return m;}
function ordered(m,bids){return [...m].sort(([a],[b])=>a===b?0:(a>b?-1:1)*(bids?1:-1)).map(([p,q])=>({price:text(p),size:text(q)}));}
function sorted(b){const bids=ordered(b.bids,true),asks=ordered(b.asks,false);need(!bids.length||!asks.length||decimal(bids[0].price,true)<=decimal(asks[0].price,true),'CROSSED_BOOK');return{bids,asks,bestBid:bids[0]?.price??null,bestAsk:asks[0]?.price??null};}
function originalTimestamp(v,received,maxFuture){if(v===undefined||v===null)return null;need(typeof v==='string'&&/^\d{1,16}$/.test(v),'SOURCE_CLOCK');const n=Number(v);need(clock(n)&&n<=received+maxFuture,'SOURCE_CLOCK');return n;}
function bookView(b,at,staleAfterMs){const valid=clock(at)&&at>=b.receivedMs&&b.sourceMs!==null&&at>=b.sourceMs&&at-b.receivedMs<=staleAfterMs&&at-b.sourceMs<=staleAfterMs;return{...sorted(b),valid,receivedAt:new Date(b.receivedMs).toISOString(),sourceTimestamp:b.sourceTimestamp,snapshotSha256:b.snapshotSha256,stateChainSha256:b.chainSha256,venueBookHash:b.venueHash??null,sequenceContinuityVerified:false,pricingAuthority:false};}
export class PolymarketMarketState{
 constructor(outcomes,{maximumAssets=1024,maximumFrameBytes=4*1024*1024,maximumLevels=10000,maximumTotalLevels=200000,staleAfterMs=30000,maxFutureSourceSkewMs=1000}={}){
  need(Number.isSafeInteger(maximumAssets)&&maximumAssets>0&&maximumAssets<=4096&&Array.isArray(outcomes)&&outcomes.length>0&&outcomes.length<=maximumAssets,'SCOPE');
  this.scope=new Map();for(const x of outcomes){need(asset(x.assetId)&&condition(x.conditionId)&&!this.scope.has(x.assetId),'SCOPE');this.scope.set(x.assetId,x.conditionId);}
  for(const n of [maximumFrameBytes,maximumLevels,maximumTotalLevels,staleAfterMs])need(Number.isSafeInteger(n)&&n>0,'CONFIG');need(maximumFrameBytes<=16*1024*1024&&maximumLevels<=100000&&maximumTotalLevels<=1000000&&staleAfterMs<=120000,'CONFIG');need(clock(maxFutureSourceSkewMs)&&maxFutureSourceSkewMs<=1000,'CONFIG');
  Object.assign(this,{maximumFrameBytes,maximumLevels,maximumTotalLevels,staleAfterMs,maxFutureSourceSkewMs});this.books=new Map();this.totalLevels=0;this.seen=new Map();this.epoch=0;this.lastReceived=0;this.requestStartedAt=null;
 }
 startEpoch(epoch,at){need(Number.isSafeInteger(epoch)&&epoch>this.epoch&&clock(at),'EPOCH');this.epoch=epoch;this.requestStartedAt=new Date(at).toISOString();this.books.clear();this.totalLevels=0;this.seen.clear();this.lastReceived=at;}
 invalidate(){const ids=[...this.books.keys()];this.books.clear();this.totalLevels=0;this.seen.clear();return ids;}
 view(assetId,at=Date.now()){
  const b=this.books.get(assetId);return b?bookView(b,at,this.staleAfterMs):null;
 }
 consume(bodyText,receivedMs){
  need(this.epoch>0&&typeof bodyText==='string'&&Buffer.byteLength(bodyText)<=this.maximumFrameBytes,'FRAME_BOUND');need(clock(receivedMs)&&receivedMs>=this.lastReceived,'RECEIPT_CLOCK');
  if(bodyText==='PONG'||bodyText==='PING')return[{eventType:'heartbeat',bodyText,bodySha256:sha(bodyText),receivedAt:new Date(receivedMs).toISOString(),streamEpoch:this.epoch,source:'polymarket_clob_ws_public',pricingAuthority:false}];
  let rows;try{rows=JSON.parse(bodyText);}catch{throw Error('POLY_WS_JSON');}rows=Array.isArray(rows)?rows:[rows];need(rows.length>0&&rows.length<=2048,'EVENT_BOUND');
  const digest=sha(bodyText),duplicate=this.seen.has(digest),patches=new Map(),events=[];
  // Transactional overlay: untouched books are neither copied nor scanned.
  // Publish only after the WHOLE original frame passes validation, including
  // the aggregate bound. null is a staged deletion, not missing authority.
  const get=id=>patches.has(id)?patches.get(id):this.books.get(id);
  const count=b=>b?b.bids.size+b.asks.size:0;
  const target=(id,market)=>{need(this.scope.get(id)===market,'ASSET_CONDITION_SCOPE');};
  const edit=id=>{const b=get(id);if(!b)return null;if(!patches.has(id))patches.set(id,{...b,bids:new Map(b.bids),asks:new Map(b.asks)});return patches.get(id);};
  for(const [eventIndex,e]of rows.entries()){
   need(e&&typeof e==='object'&&!Array.isArray(e)&&typeof e.event_type==='string'&&condition(e.market),'SCHEMA');
   const sourceMs=originalTimestamp(e.timestamp,receivedMs,this.maxFutureSourceSkewMs),type=e.event_type;
   const emit=(id,extra={})=>events.push({source:'polymarket_clob_ws_public',assetId:id,conditionId:e.market,streamEpoch:this.epoch,requestStartedAt:this.requestStartedAt,receivedAt:new Date(receivedMs).toISOString(),sourceTimestamp:e.timestamp??null,sourceAheadAtReceiptMs:sourceMs===null?null:Math.max(0,sourceMs-receivedMs),bodyText,bodySha256:digest,eventIndex,eventType:type,duplicate,...(!duplicate&&['book','price_change'].includes(type)&&get(id)?{normalized:bookView(get(id),receivedMs,this.staleAfterMs)}:{}),...extra,pricingAuthority:false});
   const apply=(id,fn)=>{target(id,e.market);const prior=get(id);need(!prior||sourceMs===null||prior.sourceMs===null||sourceMs>=prior.sourceMs,'OUT_OF_ORDER');if(duplicate){emit(id);return;}fn();};
   if(type==='book'){
    need(asset(e.asset_id),'ASSET');const bids=levels(e.bids,this.maximumLevels),asks=levels(e.asks,this.maximumLevels);
    apply(e.asset_id,()=>{const b={bids,asks,receivedMs,sourceMs,sourceTimestamp:e.timestamp??null,snapshotSha256:digest,chainSha256:sha(`${digest}:${eventIndex}:${e.asset_id}`),venueHash:e.hash??null};patches.set(e.asset_id,b);emit(e.asset_id);});
   }else if(type==='price_change'){
    need(Array.isArray(e.price_changes)&&e.price_changes.length>0&&e.price_changes.length<=this.scope.size*2+10000,'CHANGE_BOUND');const changed=new Set();
    for(const c of e.price_changes){need(c&&asset(c.asset_id)&&['BUY','SELL'].includes(c.side),'CHANGE');target(c.asset_id,e.market);const p=decimal(c.price,true),q=decimal(c.size);for(const k of ['best_bid','best_ask'])if(c[k]!==undefined&&c[k]!==null)decimal(c[k],true);
     const prior=get(c.asset_id);need(!prior||sourceMs===null||prior.sourceMs===null||sourceMs>=prior.sourceMs,'OUT_OF_ORDER');
     if(!duplicate&&prior){const b=edit(c.asset_id),side=c.side==='BUY'?b.bids:b.asks;if(q===0n)side.delete(p);else side.set(p,q);need(side.size<=this.maximumLevels,'LEVEL_BOUND');b.receivedMs=receivedMs;b.sourceMs=sourceMs;b.sourceTimestamp=e.timestamp??null;b.chainSha256=sha(`${b.chainSha256}:${digest}:${eventIndex}:${c.asset_id}`);}
     changed.add(c.asset_id);
    }for(const id of changed)emit(id,{snapshotRequired:!get(id)});
   }else if(['last_trade_price','tick_size_change','best_bid_ask'].includes(type)){
    need(asset(e.asset_id),'ASSET');target(e.asset_id,e.market);
    if(type==='last_trade_price'){decimal(e.price,true);if(e.size!==undefined)decimal(e.size);if(e.fee_rate_bps!==undefined)decimal(e.fee_rate_bps);need(['BUY','SELL'].includes(e.side),'TRADE_SIDE');}
    if(type==='tick_size_change'){need(decimal(e.new_tick_size,true)>0n,'TICK');if(e.old_tick_size!==undefined)need(decimal(e.old_tick_size,true)>0n,'TICK');}
    if(type==='best_bid_ask')for(const k of ['best_bid','best_ask','spread'])if(e[k]!==undefined&&e[k]!==null)decimal(e[k],true);
    // Auxiliary messages cannot renew or manufacture a complete book.
    emit(e.asset_id);
   }else if(type==='market_resolved'||type==='new_market'){
    need(Array.isArray(e.assets_ids)&&e.assets_ids.length>0&&e.assets_ids.length<=this.scope.size,'LIFECYCLE');for(const id of e.assets_ids){target(id,e.market);if(type==='market_resolved')patches.set(id,null);emit(id,{marketResolved:type==='market_resolved'});}
    if(type==='market_resolved')need(e.assets_ids.includes(e.winning_asset_id),'RESOLUTION');
   }else throw Error('POLY_WS_EVENT_TYPE');
  }
  let total=this.totalLevels;for(const [id,b]of patches)total+=count(b)-count(this.books.get(id));
  need(total<=this.maximumTotalLevels,'TOTAL_LEVEL_BOUND');
  for(const [id,b]of patches)if(b)this.books.set(id,b);else this.books.delete(id);
  this.totalLevels=total;this.lastReceived=receivedMs;if(!duplicate){this.seen.set(digest,true);if(this.seen.size>4096)this.seen.delete(this.seen.keys().next().value);}
  return events;
 }
}
export function startPolymarketMarketStream({outcomes,onEvent=async()=>{},onStatus=()=>{},webSocketFactory=(url,options)=>new WebSocket(url,options),now=Date.now,maximumPendingBytes=64*1024*1024,maximumPendingFrames=1024,customFeatureEnabled=false,heartbeatMs=10000,heartbeatTimeoutMs=30000,reconnectBaseMs=1000,reconnectMaximumMs=60000,setIntervalFn=setInterval,clearIntervalFn=clearInterval,setTimeoutFn=setTimeout,clearTimeoutFn=clearTimeout,...stateOptions}){
 need(typeof customFeatureEnabled==='boolean'&&heartbeatMs===10000&&Number.isSafeInteger(maximumPendingBytes)&&maximumPendingBytes>0&&Number.isSafeInteger(maximumPendingFrames)&&maximumPendingFrames>0&&maximumPendingFrames<=4096&&maximumPendingBytes<=64*1024*1024,'CONFIG');for(const n of [heartbeatTimeoutMs,reconnectBaseMs,reconnectMaximumMs])need(Number.isSafeInteger(n),'CONFIG');need(heartbeatTimeoutMs>=20000&&heartbeatTimeoutMs<=120000&&reconnectBaseMs>=1&&reconnectMaximumMs>=reconnectBaseMs&&reconnectMaximumMs<=60000,'CONFIG');
 const state=new PolymarketMarketState(outcomes,stateOptions);let stopped=false,socket,timer,retry,epoch=0,attempt=0,pendingBytes=0,pendingFrames=0,tail=Promise.resolve(),requestAt=0,lastPong=0,stopConnection;
 const stats={connections:0,reconnects:0,frames:0,invalidFrames:0,droppedFrames:0,lastReason:null};
 const status=()=>({...stats,streamEpoch:epoch,books:state.books.size,pendingBytes,pendingFrames,stopped,pricingAuthority:false});
 const report=()=>{try{onStatus(status());}catch{stopped=true;socket?.close();}};
 const enqueue=fn=>{tail=tail.then(fn).catch(()=>{stopped=true;state.invalidate();stats.lastReason='SINK_ERROR';socket?.close();report();});};
 const connect=()=>{
  if(stopped)return;requestAt=now();need(clock(requestAt),'RECEIPT_CLOCK');let ended=false,opened=false,ws,frameIndex=0;
  const end=reason=>{if(ended)return;ended=true;const endedEpoch=epoch,endedAt=now();clearIntervalFn(timer);state.invalidate();stats.lastReason=reason;
   enqueue(async()=>{for(const x of outcomes)await onEvent({source:'polymarket_clob_ws_public',assetId:x.assetId,conditionId:x.conditionId,streamEpoch:endedEpoch,frameIndex:null,eventType:'stream_invalidated',receivedAt:new Date(endedAt).toISOString(),sourceTimestamp:null,bodyText:null,bodySha256:null,normalized:{valid:false,sequenceContinuityVerified:false},pricingAuthority:false});});
   try{ws?.terminate?.();}catch{}report();if(!stopped){stats.reconnects++;const delay=Math.min(reconnectMaximumMs,reconnectBaseMs*2**Math.min(attempt++,16));retry=setTimeoutFn(connect,delay);retry?.unref?.();}
  };
  stopConnection=end;try{ws=webSocketFactory(MARKET_WS_URL,{maxPayload:state.maximumFrameBytes,followRedirects:false,perMessageDeflate:false});socket=ws;}catch{end('SOCKET_CREATE');return;}
  ws.on('open',()=>{if(stopped||ended)return;opened=true;epoch++;stats.connections++;state.startEpoch(epoch,requestAt);lastPong=now();try{ws.send(JSON.stringify({assets_ids:outcomes.map(x=>x.assetId),type:'market',...(customFeatureEnabled?{custom_feature_enabled:true}:{})}));}catch{end('SUBSCRIBE_SEND');return;}
   timer=setIntervalFn(()=>{if(stopped||ended)return;if(now()-lastPong>heartbeatTimeoutMs){end('HEARTBEAT_TIMEOUT');return;}try{ws.send('PING');}catch{end('HEARTBEAT_SEND');}},heartbeatMs);timer?.unref?.();report();});
  ws.on('message',data=>{
   if(stopped||ended||!opened)return;const at=now(),raw=Buffer.isBuffer(data)?data.toString('utf8'):String(data),bytes=Buffer.byteLength(raw),frameEpoch=epoch,ordinal=++frameIndex;
   if(Buffer.isBuffer(data)&&!Buffer.from(raw,'utf8').equals(data)){stats.invalidFrames++;end('UTF8_FRAME');return;}
   if(bytes>state.maximumFrameBytes||pendingBytes+bytes>maximumPendingBytes||pendingFrames>=maximumPendingFrames){stats.droppedFrames++;end('PENDING_OR_FRAME_BOUND');return;}
   if(raw==='PONG')lastPong=at;pendingBytes+=bytes;pendingFrames++;enqueue(async()=>{try{if(ended||frameEpoch!==epoch){stats.droppedFrames++;return;}stats.frames++;let events;try{events=state.consume(raw,at);}catch(error){stats.invalidFrames++;await onEvent({source:'polymarket_clob_ws_public',streamEpoch:frameEpoch,frameIndex:ordinal,eventType:'invalid_frame',receivedAt:new Date(at).toISOString(),sourceTimestamp:null,bodyText:raw,bodySha256:sha(raw),validationCode:/^POLY_WS_[A-Z_]+$/.test(error?.message??'')?error.message:'POLY_WS_SCHEMA',pricingAuthority:false});end('INVALID_FRAME');return;}
     if(raw==='PING')ws.send('PONG');for(const event of events)await onEvent({...event,frameIndex:ordinal});
    }finally{pendingBytes-=bytes;pendingFrames--;}});
  });
  ws.on('error',()=>end('SOCKET_ERROR'));ws.on('close',()=>end('SOCKET_CLOSED'));
 };
 connect();return{status,view:(id,at=now())=>state.view(id,at),async stop(){stopped=true;clearTimeoutFn(retry);clearIntervalFn(timer);stopConnection?.('STOPPED');state.invalidate();try{socket?.terminate?.();}catch{}await new Promise(resolve=>{const deadline=setTimeout(resolve,5000);tail.then(()=>{clearTimeout(deadline);resolve();},()=>{clearTimeout(deadline);resolve();});});report();}};
}
