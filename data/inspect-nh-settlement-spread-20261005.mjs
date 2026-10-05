import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {price,money,SCALE,TICK} from '../paper/fixed.mjs';
const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime/production-metadata/nh-settlement-spread-20261005-v1');
const legs=['381','382'].map(marketId=>{
 const files=[marketId+'.json',marketId+'.book.json'],bytes=files.map(f=>fs.readFileSync(path.join(dir,f)));
 const [node,receipt]=bytes.map(b=>JSON.parse(b));
 return{marketId,exchangeId:receipt.book.exchangeId,contract:node.root,contractReceivedAt:node.receivedAt,
  requestStartedAt:receipt.book.requestStartedAt,receivedAt:receipt.book.receivedAt,
  topBid:receipt.book.bids[0],topAsk:receipt.book.asks[0],
  sources:files.map((file,i)=>({file,sha256:createHash('sha256').update(bytes[i]).digest('hex')}))};
});
const a=legs[0].contract.contract_details,b=legs[1].contract.contract_details;
const commonPredicate=['raceId','stageId','usState','raceName','raceStage','officeLevel','contractType','electionDate','resolutionType'].every(k=>a[k]===b[k]);
if(!commonPredicate||a.winnerName===b.winnerName||legs.some(l=>l.contract.settled_with!==null))throw Error('CONTRACT_PAIR_CHANGED');
const noCost=legs.reduce((sum,l)=>sum+SCALE-price(l.topBid.price)+TICK,0n);
const yesCost=legs.reduce((sum,l)=>sum+price(l.topAsk.price)+TICK,0n);
const windowMs=Math.max(...legs.map(l=>Date.parse(l.receivedAt)))-Math.min(...legs.map(l=>Date.parse(l.requestStartedAt)));
const out={schemaVersion:1,at:new Date().toISOString(),mode:'scoped_structural_settlement_price_diagnostic',legs,
 commonPredicate,distinctPartyPredicates:true,fees:'0.000000',entrySlippagePerLeg:'0.005000',
 indicativeTwoNoCost:money(noCost),conditionalOrdinaryTwoNoMinimumPayout:'1.000000',
 indicativeConditionalNoSurplus:money(SCALE-noCost),indicativeTwoYesCost:money(yesCost),
 requestWindowMs:windowMs,cofreshExecutableBooks:windowMs<=10000,
 reviewedRelationshipStillCurrent:false,executionAuthority:false,competitionOrders:0,profitabilityEstablished:false,
 limitations:['The exact contract predicates match, but ordinary mutually exclusive settlement is conditional; administrative overrides and other reviewed exceptions are not excluded.',
  'These auxiliary observations are not cofresh execution evidence. No dispatch or package fill is inferred.',
  'Refund returns the held refundable cost rather than a fixed face-value payout. Asymmetric cancellation and unmatched legs can lose principal.',
  'The older ballot/review attestation is expired; fresh API nodes alone do not renew it.'],
 primaryRules:['https://sig.thesuper.market/docs/settlement-and-payouts','https://sig.thesuper.market/docs/markets-and-trading']};
fs.writeFileSync('data/nh-settlement-spread-diagnostic-20261005.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});
console.log({noCost:out.indicativeTwoNoCost,noSurplus:out.indicativeConditionalNoSurplus,yesCost:out.indicativeTwoYesCost,requestWindowMs:windowMs,cofreshExecutableBooks:out.cofreshExecutableBooks,competitionOrders:0});
