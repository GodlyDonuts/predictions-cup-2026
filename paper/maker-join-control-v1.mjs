// Explicit comparison control. It joins displayed touch prices; it does not
// infer a queue fill, predict alpha, or overwrite an inventory-skew policy.
import {amount,money,price,SCALE} from './fixed.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {MAKER_SCHEDULER_POLICY_V1} from '../execution/maker-quote-scheduler-v1.mjs';
export const MAKER_JOIN_CONTROL_SCHEDULER_V1=Object.freeze({...MAKER_SCHEDULER_POLICY_V1,minimumNetEntryEdge:'0.000001'});
export function makerJoinControlV1({inventoryProposal,book}) {
  const need=(v,c)=>{if(!v)throw Error('MAKER_JOIN_'+c);};
  const {proposalHash,...parent}=inventoryProposal;
  need(payloadHash(parent)===proposalHash&&parent.marketId===book.marketId&&parent.exchangeId===book.exchangeId,'PARENT');
  const bid=price(book.bids?.[0]?.price),ask=price(book.asks?.[0]?.price);need(bid<ask,'BOOK');
  const reprice=q=>{
    need(['YES','NO'].includes(q.side)&&['buy','sell'].includes(q.action)&&Number.isSafeInteger(q.quantity)&&q.quantity>0,'QUOTE');
    const limit=q.side==='YES'?(q.action==='buy'?bid:ask):(q.action==='buy'?SCALE-ask:SCALE-bid);
    return {...q,limit:money(limit)};
  };
  const buys=parent.buys.map(reprice),sells=parent.sells.map(reprice);
  const body={schemaVersion:1,mode:'join_touch_comparison_control_v1',at:parent.at,marketId:parent.marketId,exchangeId:parent.exchangeId,
    microprice:parent.microprice,buys,sells,parentProposalHash:proposalHash,bookHash:payloadHash(book),
    maximumQuoteReservation:money(buys.reduce((n,q)=>n+amount(q.limit)*BigInt(q.quantity),0n)),
    requiresSharedFundingAllocation:true,pricePolicy:'join_touch_without_inventory_price_skew',
    executionAuthority:false,networkRequests:0,profitabilityEstablished:false};
  return {...body,proposalHash:payloadHash(body)};
}
