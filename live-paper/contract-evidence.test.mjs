import test from 'node:test';import assert from 'node:assert/strict';
import {projectContractPairEvidence,createContractEvidenceReader} from './contract-evidence.mjs';
import {ReadClient} from './transport-runtime.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',start=Date.parse('2026-10-02T20:00:00.000Z');
const legs=[{marketId:'381',exchangeId:'1070',contractId:'382'},{marketId:'382',exchangeId:'1071',contractId:'383'}];
const clock=n=>new Date(start+n).toISOString();
function fixture(){return {tournamentId:tid,legs,startedAt:clock(0),finishedAt:clock(4000),responses:legs.flatMap((s,i)=>{
  const contexts=[{type:'tournament',tournament:{id:tid},status:'open',settledWith:null,settledOn:null,exchanges:[{id:s.exchangeId}]}];
  const predicate={raceId:'62972',stageId:'98102',usState:'NH',raceName:'U.S. Senate New Hampshire',raceStage:'General',winnerName:i?'Republican Party':'Democratic Party',officeLevel:'Federal',contractType:'Election Outcome',electionDate:'2026-11-03',resolutionType:'Party Winner'};
  return [{endpoint:`/markets/${s.marketId}?tournamentId=${tid}`,status:200,requestStartedAt:clock(i*2000),receivedAt:clock(i*2000+1000),data:{id:s.marketId,status:'open',settledWith:null,settledOn:null,settlementDate:'2026-11-04T17:00:00.000Z',isComposite:false,isMultiOutcome:false,exchanges:[{id:s.exchangeId}],contexts}},
    {endpoint:`/markets/${s.marketId}/nodes?tournamentId=${tid}`,status:200,requestStartedAt:clock(i*2000+1000),receivedAt:clock(i*2000+2000),data:{market_id:s.marketId,contexts,root:{node_type:'contract',node_id:s.contractId,contract_id:s.contractId,contract_type:'Election Outcome',settlement_date:'2026-11-04T17:00:00.000Z',settled_with:null,contract_details:predicate}}}];})};}
test('current explicit-scoped leaf predicates form a conditional inference, never an order authority',()=>{
  const p=projectContractPairEvidence(fixture(),start+4000);assert.equal(p.conditionalNormalMutualExclusion,true);
  assert.equal(p.orderEligible,false);assert.equal(p.engineRelationshipProof,false);assert.equal(p.validUntil,clock(120000));
  assert.equal(p.legs.length,2);assert.equal(p.legs[0].sourceHashes.length,2);assert.ok(Object.isFrozen(p.legs[0].predicate));
});
test('context, exchange, settlement override and node mismatch cannot be hidden by matching titles',()=>{
  const changes=[x=>x.responses[0].data.contexts[0].tournament.id='other',x=>x.responses[0].data.exchanges[0].id='999',
    x=>x.responses[0].data.settledWith='YES',x=>x.responses[0].data.contexts[0].status='settled',
    x=>x.responses[1].data.root.node_type='operator',x=>x.responses[1].data.root.contract_id='999',
    x=>x.responses[1].data.root.settled_with='NO',x=>x.responses[0].data.isMultiOutcome=true];
  for(const mutate of changes){const x=fixture();mutate(x);assert.throws(()=>projectContractPairEvidence(x,start+4000));}
});
test('different stage, election date, resolution rule, party or settlement date blocks pair inference',()=>{
  for(const [key,value]of Object.entries({stageId:'999',electionDate:'2026-11-05',resolutionType:'Candidate Winner',winnerName:'Democratic Party'})){
    const x=fixture();x.responses[3].data.root.contract_details[key]=value;assert.throws(()=>projectContractPairEvidence(x,start+4000));}
  const x=fixture();x.responses[3].data.root.settlement_date='2026-11-05T17:00:00.000Z';assert.throws(()=>projectContractPairEvidence(x,start+4000));
});
test('old, future, incomplete or overlapping receipt evidence rejects',()=>{
  assert.throws(()=>projectContractPairEvidence(fixture(),start+120001),/STALE/);
  assert.throws(()=>projectContractPairEvidence(fixture(),start+3999),/STALE/);
  const x=fixture();x.responses.pop();assert.throws(()=>projectContractPairEvidence(x,start+4000),/INCOMPLETE/);
  const y=fixture();y.responses[2].requestStartedAt=clock(1000);assert.throws(()=>projectContractPairEvidence(y,start+4000),/CLOCK/);
});
test('sole-client scoped permits exist only for the four serialized dispatches and expire afterwards',async()=>{
  const x=fixture(),specs=[];let t=start,reader;
  reader=createContractEvidenceReader({tournamentId:tid,legs,now:()=>t,dispatch:async spec=>{
    assert.equal(reader.authorizes(spec),true);assert.equal(reader.authorizes({...spec,method:'POST'}),false);
    assert.equal(reader.authorizes({...spec,endpoint:spec.endpoint+'&other=1'}),false);
    assert.equal(reader.authorizes({...spec,headers:{}}),false);spec.beforeNetwork();specs.push(spec);
    const r=x.responses[specs.length-1];t=Date.parse(r.receivedAt);return r;
  }});
  const p=await reader.capture();assert.equal(specs.length,4);assert.equal(p.evidenceHash,projectContractPairEvidence(x,start+4000).evidenceHash);
  for(const spec of specs)assert.equal(reader.authorizes(spec),false);
});
test('STOP while queued or after receipt revokes permission and publishes no partial success',async()=>{
  let enabled=true,called=0,reader;
  reader=createContractEvidenceReader({tournamentId:tid,legs,now:()=>start,canOperate:()=>enabled,dispatch:async spec=>{
    called++;enabled=false;assert.equal(reader.authorizes(spec),false);assert.throws(()=>spec.beforeNetwork(),/STOPPED/);return fixture().responses[0];
  }});
  await assert.rejects(reader.capture(),/STOPPED/);assert.equal(called,1);
  await assert.rejects(reader.capture(),/STOPPED/);assert.equal(called,1);
});
test('concurrent capture is refused and dispatch failures release the local busy flag',async()=>{
  let reject;const reader=createContractEvidenceReader({tournamentId:tid,legs,now:()=>start,dispatch:()=>new Promise((_,r)=>{reject=r;})});
  const pending=reader.capture();await assert.rejects(reader.capture(),/BUSY/);reject(Error('offline'));await assert.rejects(pending,/offline/);
  const next=reader.capture();reject(Error('offline'));await assert.rejects(next,/offline/);
});
test('actual shared transport admits only reader-owned GETs with completion pacing',async()=>{
  const x=fixture(),calls=[];let now=start,reader;
  const client=new ReadClient('SYNTHETIC',{now:()=>now,sleepFn:async ms=>{now+=ms;},
    networkBudget:{schemaVersion:2,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,formerWriterStopCommentId:5947535773},
    authorizeDispatch:s=>reader?.authorizes(s)===true,fetchFn:async(url,options)=>{
      const endpoint=url.slice('https://sig.thesuper.market/api/v1'.length),r=x.responses[calls.length];
      assert.equal(options.method,'GET');assert.equal(options.body,undefined);assert.equal(endpoint,r.endpoint);
      calls.push({at:now,endpoint});return {ok:true,status:200,json:async()=>{now+=100;return r.data;}};
    }});
  try{
    reader=createContractEvidenceReader({tournamentId:tid,legs,now:()=>now,dispatch:s=>client.dispatch(s)});
    const p=await reader.capture();assert.equal(p.legs.length,2);assert.equal(calls.length,4);
    assert.deepEqual(calls.map(c=>c.at-start),[0,1100,2200,3300]);
    assert.throws(()=>client.dispatch({method:'GET',endpoint:x.responses[0].endpoint}),/not authorized/);
    assert.equal(p.executionAuthority,false);
  }finally{client.close();}
});
