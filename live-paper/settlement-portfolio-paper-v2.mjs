import {digest} from './model.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {amount,money,instant} from '../paper/fixed.mjs';
import {validateResearch} from './probability-paper.mjs';
import {SETTLEMENT_PAPER_FILLS,advanceSettlementPaper,assertSettlementPaperState,settlementPaperReport} from './settlement-paper.mjs';
const need=(v,c)=>{if(!v)throw Error('SETTLEMENT_PORTFOLIO_'+c);};
export const PORTFOLIO_POLICY=Object.freeze({minimumEdge:'0.020000',maximumSpread:'0.020000',maximumBookAgeMs:20000,maximumResearchAgeMs:21600000,fractionalKellyBps:2500,slippageTicks:1,depthFractionBps:100,maxOrderCost:'1.000000',maxExposure:'3.000000',cashReserve:'90.000000',cumulativeBuyBudget:'100.000000'});
export function eventRiskGroup(root){const p=root.contract_details;need(p&&['raceId','stageId','usState','raceStage','officeLevel','electionDate','resolutionType'].every(k=>typeof p[k]==='string'&&p[k]),'EVENT');return 'race_'+payloadHash(Object.fromEntries(['raceId','stageId','usState','raceStage','officeLevel','electionDate','resolutionType'].map(k=>[k,p[k]]))).slice(0,24);}
export function portfolioPolicy(researchEvidence){return{...PORTFOLIO_POLICY,groupCaps:{national_election:'3.000000',...Object.fromEntries(researchEvidence.map(e=>[eventRiskGroup(e.root),'1.000000']))}};}
export function validateSettlementPortfolioBank(bank){
 need(bank?.schemaVersion===2&&bank.mode==='funded_settlement_portfolio_paper_v2'&&Array.isArray(bank.research)&&bank.research.length>=1&&bank.research.length<=3,'BANK');
 need(Array.isArray(bank.researchEvidence)&&bank.researchEvidence.length===bank.research.length&&Object.keys(bank.identities).length===bank.research.length,'SCOPE');
 need(digest(bank.policy)===digest(portfolioPolicy(bank.researchEvidence))&&digest(bank.fillPolicy)===digest(SETTLEMENT_PAPER_FILLS),'POLICY');instant(bank.cutoff);
 const seen=new Set(),probabilities=new Map();for(const r of bank.research){validateResearch(r);const e=bank.researchEvidence.find(e=>e.researchId===r.id&&e.marketId===r.marketId),root=e?.root,id=bank.identities[r.marketId];
  need(!seen.has(r.marketId)&&r.side==='YES'&&instant(r.recordedAt)<=instant(bank.cutoff)&&id?.exchangeId===r.exchangeId&&id.predicateHash===r.predicateHash&&bank.universe.exchanges.some(x=>x.marketId===r.marketId&&x.exchangeId===r.exchangeId&&x.option==='YES'),'RESEARCH_BINDING');seen.add(r.marketId);
  need(root?.node_type==='contract'&&root.contract_type==='Election Outcome'&&root.settled_with===null&&root.contract_details?.raceStage==='General'&&root.contract_details.resolutionType==='Party Winner'&&payloadHash(root)===r.predicateHash&&e.rootHash===r.predicateHash,'EXACT_ROOT');
  need(root.contract_id===root.node_id&&/^[1-9]\d*$/.test(root.contract_id)&&instant(root.settlement_date)===bank.valuationDeadlineMs&&bank.valuationDeadlineMs>instant(bank.cutoff),'RESOLUTION_HORIZON');
  need(digest(bank.riskGroups[r.marketId])===digest(['national_election',eventRiskGroup(root)]),'EVENT_GROUP_BINDING');
  const group=eventRiskGroup(root),winner=root.contract_details.winnerName;need(typeof winner==='string'&&winner.length>0,'WINNER');const winners=probabilities.get(group)||new Map(),prior=winners.get(winner),lower=amount(r.lower),upper=amount(r.upper);winners.set(winner,{lower:prior&&prior.lower>lower?prior.lower:lower,upper:prior&&prior.upper<upper?prior.upper:upper});probabilities.set(group,winners);
 }
 for(const winners of probabilities.values()){need([...winners.values()].every(p=>p.lower<=p.upper),'CONFLICTING_DUPLICATE_VALUE');need([...winners.values()].reduce((n,p)=>n+p.lower,0n)<=1000000n,'INCOHERENT_EVENT_PROBABILITY');}
 return bank;
}
export function createSettlementPortfolioState(bank){validateSettlementPortfolioBank(bank);return{bankHash:digest(bank),lastAvailableAt:bank.cutoff,barriers:{global:bank.cutoff,markets:{}},counts:{},decisions:[],books:{},wallets:Object.fromEntries(bank.fillPolicy.latenciesMs.map(ms=>[ms,{cash:bank.fillPolicy.initialCash,realizedPnl:'0.000000',cumulativeBuys:'0.000000',positions:{},pending:{},fills:[],consumedVersions:{},lastExitedAtMs:{},intents:0}]))};}
export function assertSettlementPortfolioState(state,bank){validateSettlementPortfolioBank(bank);need(digest(Object.keys(state.wallets).sort())===digest(bank.fillPolicy.latenciesMs.map(String).sort()),'WALLET_SCOPE');assertSettlementPaperState(state,bank);for(const w of Object.values(state.wallets)){
 for(const f of w.fills)need(bank.identities[f.marketId]?.exchangeId===f.exchangeId&&f.instrument===f.exchangeId+':YES','FILL_SCOPE');
 const used=new Map();for(const [id,position]of Object.entries(w.positions)){need(bank.identities[id],'UNBOUND_POSITION');for(const group of bank.riskGroups[id])used.set(group,(used.get(group)||0n)+amount(position.cost));}
 for(const [id,intent]of Object.entries(w.pending)){need(bank.identities[id]&&intent.limit&&amount(intent.limit)<=amount(bank.policy.maxOrderCost),'UNBOUND_INTENT');if(intent.action==='buy')for(const group of bank.riskGroups[id])used.set(group,(used.get(group)||0n)+amount(intent.limit));}
 for(const [group,value]of used)need(value<=amount(bank.policy.groupCaps[group]),'CORRELATED_FUNDING');
 }return state;}
// Immutable reduction: a failed evaluator or reservation invariant leaves the
// prior published wallet unchanged. Existing V1 bank/code remain untouched.
export function advanceSettlementPortfolio(state,row,bank,availableAt,account,evaluate){assertSettlementPortfolioState(state,bank);const next=structuredClone(state),keys=Object.keys(next.wallets);let evaluated=0;
 advanceSettlementPaper(next,row,bank,availableAt,account,input=>{
  const w=next.wallets[keys[evaluated++]];need(w,'LATENCY_SCOPE');
  // Re-evaluate this pending intent without double-reserving itself, while
  // retaining every OTHER market's pending capital and event exposure.
  input.wallet.reservations=Object.entries(w.pending).filter(([id,p])=>id!==row.payload.book.marketId&&p.action==='buy').map(([id,p])=>({candidateId:id+':YES',maximumCost:p.limit}));
  const output=evaluate(input);need(output&&typeof output.then!=='function','SYNC_EVALUATOR');return output;
 });
 need(evaluated===0||evaluated===keys.length,'LATENCY_COUNT');assertSettlementPortfolioState(next,bank);return next;
}
export function settlementPortfolioReport(state,bank,at){assertSettlementPortfolioState(state,bank);const r=settlementPaperReport(state,bank,at);return{...r,stateHash:digest(state),sharedCashPerLatency:true,independentLatencySamples:false,eventGroups:Object.fromEntries(bank.research.map(x=>[x.marketId,bank.riskGroups[x.marketId]])),maximumMarkets:3,limitations:[...r.limitations,'One funded cash ledger covers all markets within each latency case; correlated cases are never summed.','Exact event groups include both held cost and every pending purchase; source probabilities are not guaranteed returns.']};}
