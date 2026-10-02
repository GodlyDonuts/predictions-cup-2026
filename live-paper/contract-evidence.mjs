import {payloadHash} from '../execution/core.mjs';

const need = (ok, code) => { if (!ok) throw Error(code); };
const id = x => typeof x === 'string' && /^[1-9]\d*$/.test(x) && Number.isSafeInteger(Number(x));
const uuid = x => typeof x === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(x);
const stamp = x => typeof x === 'string' && Number.isFinite(Date.parse(x));
const text = x => typeof x === 'string' && x.length > 0 && x.length <= 300;
const freeze = x => { if (x && typeof x === 'object') { Object.values(x).forEach(freeze); Object.freeze(x); } return x; };

function context(rows, tournamentId) {
  need(Array.isArray(rows) && rows.length === 1 && rows[0]?.type === 'tournament'
    && rows[0].tournament?.id === tournamentId, 'CONTRACT_CONTEXT_MISMATCH');
  return rows[0];
}

function receipt(response, endpoint, after, now) {
  need(response?.endpoint === endpoint && response.status === 200 && stamp(response.requestStartedAt)
    && stamp(response.receivedAt), 'CONTRACT_RECEIPT_INVALID');
  const request = Date.parse(response.requestStartedAt), received = Date.parse(response.receivedAt);
  need(request >= after && received >= request && received <= now && received - request <= 20_000,
    'CONTRACT_RECEIPT_CLOCK_INVALID');
  return {endpoint, requestStartedAt:response.requestStartedAt, receivedAt:response.receivedAt};
}

function leg(marketResponse, nodesResponse, scope, tournamentId, after, now) {
  const marketEndpoint = `/markets/${scope.marketId}?tournamentId=${tournamentId}`;
  const nodeEndpoint = `/markets/${scope.marketId}/nodes?tournamentId=${tournamentId}`;
  const marketReceipt = receipt(marketResponse,marketEndpoint,after,now);
  const nodesReceipt = receipt(nodesResponse,nodeEndpoint,Date.parse(marketReceipt.receivedAt),now);
  const m = marketResponse.data, n = nodesResponse.data, root = n?.root;
  need(m?.id === scope.marketId && n?.market_id === scope.marketId, 'CONTRACT_MARKET_MISMATCH');
  const c = context(m.contexts,tournamentId); context(n.contexts,tournamentId);
  need(m.status === 'open' && m.settledWith === null && m.settledOn === null
    && c.status === 'open' && c.settledWith === null && c.settledOn === null,
    'CONTRACT_ALREADY_CLOSED_OR_SETTLED');
  need(m.isComposite === false && m.isMultiOutcome === false && Array.isArray(m.exchanges)
    && m.exchanges.length === 1 && m.exchanges[0].id === scope.exchangeId
    && Array.isArray(c.exchanges) && c.exchanges.length === 1 && c.exchanges[0].id === scope.exchangeId,
    'CONTRACT_EXCHANGE_MISMATCH');
  need(root?.node_type === 'contract' && root.node_id === scope.contractId && root.contract_id === scope.contractId
    && root.contract_type === 'Election Outcome' && root.settled_with === null
    && stamp(root.settlement_date) && stamp(m.settlementDate)
    && Date.parse(root.settlement_date) === Date.parse(m.settlementDate)
    && Date.parse(root.settlement_date) > now, 'CONTRACT_NODE_INVALID');
  const d = root.contract_details;
  need(d && ['raceId','stageId','usState','raceName','raceStage','winnerName','officeLevel','contractType','electionDate','resolutionType']
    .every(k=>text(d[k])) && id(d.raceId) && id(d.stageId)
    && d.contractType === 'Election Outcome' && d.resolutionType === 'Party Winner'
    && d.raceStage === 'General' && /^\d{4}-\d\d-\d\d$/.test(d.electionDate)
    && new Date(d.electionDate+'T00:00:00Z').toISOString().slice(0,10) === d.electionDate
    && ['Democratic Party','Republican Party'].includes(d.winnerName), 'CONTRACT_PREDICATE_INVALID');
  const predicate = Object.fromEntries(['raceId','stageId','usState','raceName','raceStage','winnerName','officeLevel','contractType','electionDate','resolutionType'].map(k=>[k,d[k]]));
  return {...scope, settlementDate:root.settlement_date, predicate, receipts:[marketReceipt,nodesReceipt],
    sourceHashes:[payloadHash(m),payloadHash(n)]};
}

export function projectContractPairEvidence({tournamentId,legs,responses,startedAt,finishedAt}, now=Date.now()) {
  need(uuid(tournamentId) && Array.isArray(legs) && legs.length === 2
    && legs.every(s=>s && ['marketId','exchangeId','contractId'].every(k=>id(s[k])))
    && ['marketId','exchangeId','contractId'].every(k=>legs[0][k] !== legs[1][k]), 'CONTRACT_PAIR_SCOPE_INVALID');
  need(Number.isSafeInteger(now) && stamp(startedAt) && stamp(finishedAt)
    && Date.parse(finishedAt) <= now && Date.parse(finishedAt) >= Date.parse(startedAt)
    && now-Date.parse(startedAt) <= 120_000, 'CONTRACT_CAPTURE_STALE');
  need(Array.isArray(responses) && responses.length === 4, 'CONTRACT_CAPTURE_INCOMPLETE');
  const captured = []; let after = Date.parse(startedAt);
  for(let i=0;i<2;i++) {
    const result=leg(responses[i*2],responses[i*2+1],legs[i],tournamentId,after,Date.parse(finishedAt));
    captured.push(result); after=Date.parse(result.receipts[1].receivedAt);
  }
  const [a,b]=captured;
  need(Date.parse(a.settlementDate) === Date.parse(b.settlementDate)
    && a.predicate.winnerName !== b.predicate.winnerName
    && Object.keys(a.predicate).filter(k=>k!=='winnerName').every(k=>a.predicate[k]===b.predicate[k]),
    'CONTRACT_PAIR_PREDICATES_DIFFER');
  const evidence={schemaVersion:1,kind:'scoped_contract_pair_evidence',tournamentId,startedAt,finishedAt,
    validUntil:new Date(Date.parse(startedAt)+120_000).toISOString(),legs:captured,
    conditionalNormalMutualExclusion:true,engineRelationshipProof:false,adminOverrideProtection:false,
    refundCompatibilityProven:false,orderEligible:false,executionAuthority:false};
  return freeze({...evidence,evidenceHash:payloadHash(evidence)});
}

// Uses the caller's sole serialized client and dispatch fencing. No credential
// storage, independent transport, order endpoint or recurring loop is created.
export function createContractEvidenceReader({tournamentId,legs,dispatch,canOperate=()=>true,now=Date.now}) {
  need(uuid(tournamentId) && Array.isArray(legs) && legs.length===2 && typeof dispatch==='function'
    && typeof canOperate==='function' && typeof now==='function', 'CONTRACT_READER_DEPENDENCIES');
  const scope=structuredClone(legs);
  need(scope.every(s=>s&&['marketId','exchangeId','contractId'].every(k=>id(s[k])))
    && ['marketId','exchangeId','contractId'].every(k=>scope[0][k]!==scope[1][k]), 'CONTRACT_PAIR_SCOPE_INVALID');
  const permits=new WeakMap();let busy=false;
  const allowed=()=>canOperate()===true;
  const authorizes=spec=>allowed() && spec?.method==='GET' && spec.body===undefined && spec.bodyJson===undefined
    && spec.headers===undefined && typeof spec.beforeNetwork==='function' && spec.beforeDispatch===spec.beforeNetwork
    && permits.get(spec.beforeNetwork)===spec.endpoint;
  async function capture() {
    need(!busy,'CONTRACT_CAPTURE_BUSY');busy=true;
    const startedAt=new Date(now()).toISOString(),responses=[];
    try {
      for(const s of scope) for(const suffix of ['','/nodes']) {
        need(allowed(),'CONTRACT_CAPTURE_STOPPED');
        const endpoint=`/markets/${s.marketId}${suffix}?tournamentId=${tournamentId}`;
        const fence=()=>{need(allowed()&&permits.get(fence)===endpoint,'CONTRACT_CAPTURE_STOPPED');return true;};
        permits.set(fence,endpoint);
        try { responses.push(await dispatch({method:'GET',endpoint,beforeDispatch:fence,beforeNetwork:fence}));fence(); }
        finally { permits.delete(fence); }
      }
      return projectContractPairEvidence({tournamentId,legs:scope,responses,startedAt,finishedAt:new Date(now()).toISOString()},now());
    } finally {busy=false;}
  }
  return Object.freeze({authorizes,capture});
}
