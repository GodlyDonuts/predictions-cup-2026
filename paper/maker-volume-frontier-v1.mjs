import {instant} from './fixed.mjs';import {payloadHash} from '../execution/ledger.mjs';
// Observed volume continuity is distinct from projection-complete retirement.
export function makerVolumeBeforePageV1(quote,page){
 if(quote.marketId!==page.marketId||quote.exchangeId!==page.exchangeId)throw Error('MAKER_VOLUME_SCOPE');
 const state=quote.observedVolume??{through:quote.activationAt,pending:{},windows:[]},key=payloadHash([page.from,page.to]),request=page.requestCursor;
 if(request!==null&&(typeof request!=='string'||!request))throw Error('MAKER_VOLUME_CURSOR');
 const gap=request===null?instant(page.from)>state.through:state.pending[key]?.expected!==request;
 return{state:structuredClone(state),resetCredit:gap};
}
export function makerVolumeAfterPageV1(state,page){
 const s=structuredClone(state),from=instant(page.from),to=instant(page.to),key=payloadHash([page.from,page.to]),request=page.requestCursor;
 if(from>=to||typeof page.coverage?.complete!=='boolean'||typeof page.pagination?.hasMore!=='boolean')throw Error('MAKER_VOLUME_PAGE');
 let chain=request===null?{from,to,expected:null,complete:true,seen:[]}:s.pending[key];
 if(!chain||chain.expected!==request){delete s.pending[key];return s;}chain.complete=chain.complete&&page.coverage.complete;
 if(page.pagination.hasMore){const next=page.pagination.nextCursor;if(typeof next!=='string'||!next||next===request||chain.seen.includes(next))throw Error('MAKER_VOLUME_CURSOR_CYCLE');chain.seen.push(next);chain.expected=next;s.pending[key]=chain;if(Object.keys(s.pending).length>16)throw Error('MAKER_VOLUME_PENDING_LIMIT');}
 else{if(page.pagination.nextCursor!==null)throw Error('MAKER_VOLUME_FINAL_CURSOR');if(chain.complete)s.windows.push({from,to});delete s.pending[key];}
 for(const w of s.windows.sort((a,b)=>a.from-b.from)){if(w.from>s.through)break;s.through=Math.max(s.through,w.to);}s.windows=s.windows.filter(w=>w.to>s.through);return s;
}
