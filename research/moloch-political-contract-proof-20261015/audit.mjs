import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const need=(condition,message)=>{if(!condition)throw Error(message);};
const numeric=s=>typeof s==='string'&&/^[1-9]\d*$/.test(s)&&BigInt(s)<=9007199254740991n;
export function identifyPair(legs){
 need(Array.isArray(legs)&&legs.length===2,'EXACT_TWO_LEGS');
 for(const l of legs){need(numeric(l.marketId)&&numeric(l.exchangeId),'LEG_SCOPE');const r=l.root,d=r?.contract_details;need(r?.node_type==='contract'&&numeric(r.node_id)&&r.node_id===r.contract_id&&r.contract_type==='Election Outcome'&&r.settled_with===null,'SIMPLE_UNSETTLED_ELECTION_ROOT');need(d?.contractType==='Election Outcome'&&d.resolutionType==='Party Winner','PARTY_WINNER_PREDICATE');for(const k of ['raceId','stageId'])need(numeric(d[k]),'CANONICAL_CONTEST_IDS');for(const k of ['usState','raceStage','officeLevel','electionDate','winnerName'])need(typeof d[k]==='string'&&d[k].length>0,'CONTEST_IDENTITY_MISSING');need(d.raceStage==='General'&&/^\d{4}-\d{2}-\d{2}$/.test(d.electionDate),'GENERAL_ELECTION_DATE');}
 const [a,b]=legs,da=a.root.contract_details,db=b.root.contract_details;
 need(a.marketId!==b.marketId&&a.exchangeId!==b.exchangeId&&a.root.contract_id!==b.root.contract_id,'DISTINCT_PREDICATES');
 for(const field of ['raceId','stageId','usState','raceStage','officeLevel','electionDate','resolutionType'])need(da[field]===db[field],`CONTEST_MISMATCH_${field}`);
 need(da.winnerName!==db.winnerName,'IDENTICAL_PARTY_NOT_EXCLUSION');
 return {sameContestMetadata:true,contest:{raceId:da.raceId,stageId:da.stageId,usState:da.usState,raceStage:da.raceStage,officeLevel:da.officeLevel,electionDate:da.electionDate},predicates:legs.map(l=>({marketId:l.marketId,exchangeId:l.exchangeId,contractId:l.root.contract_id,party:l.root.contract_details.winnerName})),ordinaryExclusionCertified:false,allStatePayoutFloorCertified:false,missingEvidence:['Current exact per-contract party-affiliation/fusion resolution text and version.','Source-backed mapping from every possible certified winner to each contract predicate.','Coupled versus independent N/A/cancellation policy and exact refundable basis.','Fresh scoped raw node/market receipts and account eligibility.'],executionAuthority:false};
}
export function micros(s){need(typeof s==='string'&&/^\d+\.\d{6}$/.test(s),'EXACT_NONNEGATIVE_MICROS');return BigInt(s.replace('.',''));}
const format=n=>`${n<0n?'-':''}${(n<0n?-n:n)/1000000n}.${((n<0n?-n:n)%1000000n).toString().padStart(6,'0')}`;
export function payoffStress(a,b){
 const pa=micros(a),pb=micros(b),cost=pa+pb;need(pa<=1000000n&&pb<=1000000n,'PRICE_RANGE');
 const scenario=(id,x,y)=>({id,payouts:[format(x),format(y)],totalPayout:format(x+y),netReturn:format(x+y-cost)});
 return {purchaseCosts:[a,b],totalCost:format(cost),ordinaryExclusionIsAssumption:true,
  scenarios:[scenario('A_party_wins_only',0n,1000000n),scenario('B_party_wins_only',1000000n,0n),scenario('another_party_wins',1000000n,1000000n),scenario('both_party_predicates_resolve_yes',0n,0n),scenario('A_refunded_B_no_loses',pa,0n),scenario('B_refunded_A_no_loses',0n,pb),scenario('both_refunded_at_original_purchase_cost',pa,pb)],
  ordinaryConditionalSurplus:format(1000000n-cost),allStatesMinimumNetReturn:format(-cost),
  zeroPayoutExceptionBreakEvenProbability:{numerator:(1000000n-cost).toString(),denominator:'1000000',estimated:false},
  notes:['Refund examples assume a newly bought fully remaining lot has refundable basis equal to its shown purchase cost. Inherited unknown basis cannot use that assumption.','The break-even ratio assumes all ordinary outcomes pay exactly one and the exceptional outcome pays zero; no exception probability is estimated.'],executionAuthority:false};
}
export function audit(repositoryRoot){
 const read=p=>{const bytes=fs.readFileSync(path.join(repositoryRoot,p));return{path:p,sha256:sha(bytes),value:JSON.parse(bytes)};};
 const review=read('data/dual-no-sprint-initialization-20261005.json');
 const screen=read('data/moloch-structural-economics-20261014-40.json');
 const pair=identifyPair(review.value.review.legs);
 const candidates=screen.value.races.map(r=>r.largestConditionalSurplus).sort((a,b)=>Number(a.packageCost)-Number(b.packageCost));
 return {schemaVersion:1,createdAt:new Date().toISOString(),sourceArtifacts:[review,screen].map(({path,sha256})=>({path,sha256})),
  historicalMichiganRootReview:{recordedAt:review.value.review.recordedAt,expiredAt:review.value.review.expiresAt,originalSourceReceiptAvailable:false,...pair},
  historicalStructuralScan:{window:screen.value.window,matchedTitlePairs:screen.value.matchedPairs,racesWithObservations:screen.value.races.length,observations:screen.value.races.reduce((n,r)=>n+r.observations,0),settlementProofEstablished:false,currentExecutionQuotes:false},
  highestHistoricalConditionalCandidates:candidates.slice(0,3).map(c=>({race:c.race,receivedAt:c.receivedAt,legs:c.legs,purchaseLimits:c.noPurchaseLimits,cost:c.packageCost,ordinaryConditionalSurplus:c.conditionalSurplus,immediateRoundTripPnl:c.immediateRoundTripPnl,settlementRulesVerified:c.settlementRulesVerified})),
  historicalMichiganCurrentScanStress:payoffStress('0.105000','0.885000'),bestHistoricalPackageStress:payoffStress('0.080000','0.875000'),
  currentProfitableTradeEstablished:false,executionAuthority:false};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const repositoryRoot=path.resolve(fileURLToPath(new URL('../../',import.meta.url)));
 const output=process.argv[2];need(output,'NEW_OUTPUT_PATH_REQUIRED');fs.writeFileSync(output,JSON.stringify(audit(repositoryRoot),null,2)+'\n',{flag:'wx'});
}
