// Proposal mapping only. The host must install the scheduler's selected quotes
// into its causal matcher; the older wallet's internally generated quotes are
// not evidence that this scheduler or batching policy executed.
import {amount,money,price,instant,SCALE} from './fixed.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const need=(v,c)=>{if(!v)throw Error('MAKER_PROPOSAL_'+c);};
const key=q=>q.side+':'+q.action;
export function adaptMakerProposalV1({proposal,book,valuations,markoutReserve=null,feePerShare='0.000000',at,lifetimeMs}) {
  const now=instant(at);need(Number.isSafeInteger(lifetimeMs)&&lifetimeMs>0&&lifetimeMs<=120000,'LIFETIME');
  const {proposalHash,...body}=proposal;
  need(proposalHash===payloadHash(body)&&proposal.at===at&&proposal.marketId===book.marketId&&proposal.exchangeId===book.exchangeId,'PROPOSAL_BINDING');
  const started=instant(book.requestStartedAt),received=instant(book.receivedAt);
  need(started<=received&&received<=now&&now-started<=10000&&typeof book.versionKey==='string'&&book.versionKey.length>0,'BOOK_CLOCK_OR_VERSION');
  need(Array.isArray(proposal.buys)&&Array.isArray(proposal.sells),'INPUT');
  let reserve=0n;
  if(markoutReserve!==null){reserve=amount(markoutReserve.perShare);need(reserve>=0n&&
    typeof markoutReserve.sourceHash==='string'&&/^[a-f0-9]{64}$/.test(markoutReserve.sourceHash)&&instant(markoutReserve.knownAt)<=now,'MARKOUT_RESERVE');}
  if(valuations===undefined){
    need(markoutReserve!==null&&Number.isFinite(proposal.microprice)&&proposal.microprice>=0&&proposal.microprice<=1,'EXPLICIT_HEURISTIC_RESERVE');
    // Quantize a model estimate, never financial cash. Both outcomes remain
    // microprice heuristics; the supplied reserve is separately source-bound.
    const yes=BigInt(Math.floor(proposal.microprice*1000000));
    valuations=[...proposal.sells,...proposal.buys].map(q=>({side:q.side,action:q.action,
      valuePerShare:money(q.side==='YES'?yes:SCALE-yes),feePerShare,valueBasis:'microprice_heuristic',knownAt:at}));
  }
  need(Array.isArray(valuations),'INPUT');
  const values=new Map();
  for(const v of valuations){
    need(['YES','NO'].includes(v.side)&&['buy','sell'].includes(v.action)&&!values.has(key(v)),'VALUATION_ID');
    const value=amount(v.valuePerShare),fee=amount(v.feePerShare);
    need(value>=0n&&value<=SCALE&&fee>=0n&&['microprice_heuristic','conditional_fill_model'].includes(v.valueBasis)&&
      instant(v.knownAt)<=now,'VALUATION');
    values.set(key(v),v);
  }
  const seen=new Set(),validUntil=new Date(now+lifetimeMs).toISOString();
  const quotes=[...proposal.sells,...proposal.buys].map(q=>{
    need(['YES','NO'].includes(q.side)&&['buy','sell'].includes(q.action)&&!seen.has(key(q))&&
      Number.isSafeInteger(q.quantity)&&q.quantity>0,'QUOTE');seen.add(key(q));
    const v=values.get(key(q));need(v,'MISSING_VALUATION');const limit=price(q.limit),value=amount(v.valuePerShare),fee=amount(v.feePerShare);
    return{marketId:book.marketId,exchangeId:book.exchangeId,side:q.side.toLowerCase(),action:q.action,quantity:q.quantity,
      price:money(limit),sourceAt:book.receivedAt,bookRequestStartedAt:book.requestStartedAt,validUntil,
      netEdgePerShare:money((q.action==='buy'?value-limit:limit-value)-fee-reserve),markoutReservePerShare:money(reserve),
      reduceOnly:q.action==='sell',valueBasis:v.valueBasis,valueKnownAt:v.knownAt,
      sourceBinding:{proposalHash,bookHash:payloadHash(book),bookVersion:book.versionKey,valuationHash:payloadHash(v),
        markoutReserveHash:markoutReserve===null?null:payloadHash(markoutReserve)}};
  });
  const out={schemaVersion:1,at,quotes,proposalHash,valuationClaims:valuations.map(v=>({key:key(v),basis:v.valueBasis})),
    executionAuthority:false,networkRequests:0,profitabilityEstablished:false};
  return {...out,adapterHash:payloadHash(out)};
}
