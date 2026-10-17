// Public discovery only. Identity hints never certify payout equivalence.
import {readFileSync,writeFileSync,mkdirSync,existsSync,renameSync} from 'node:fs';
import {resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {gunzipSync} from 'node:zlib';
import {createHash} from 'node:crypto';

const sha=b=>createHash('sha256').update(b).digest('hex');
const need=(v,c)=>{if(!v)throw Error(c);};
const STATES=Object.fromEntries('AL:Alabama|AK:Alaska|AZ:Arizona|AR:Arkansas|CA:California|CO:Colorado|CT:Connecticut|DE:Delaware|FL:Florida|GA:Georgia|HI:Hawaii|ID:Idaho|IL:Illinois|IN:Indiana|IA:Iowa|KS:Kansas|KY:Kentucky|LA:Louisiana|ME:Maine|MD:Maryland|MA:Massachusetts|MI:Michigan|MN:Minnesota|MS:Mississippi|MO:Missouri|MT:Montana|NE:Nebraska|NV:Nevada|NH:New Hampshire|NJ:New Jersey|NM:New Mexico|NY:New York|NC:North Carolina|ND:North Dakota|OH:Ohio|OK:Oklahoma|OR:Oregon|PA:Pennsylvania|RI:Rhode Island|SC:South Carolina|SD:South Dakota|TN:Tennessee|TX:Texas|UT:Utah|VT:Vermont|VA:Virginia|WA:Washington|WV:West Virginia|WI:Wisconsin|WY:Wyoming'.split('|').map(s=>s.split(':')));
const STATE_ORDER=Object.entries(STATES).sort((a,b)=>b[1].length-a[1].length);
const MAX_BODY=8*1024*1024, MAX_OUTCOMES=4096;
const uint256=s=>typeof s==='string'&&/^(0|[1-9]\d{0,77})$/.test(s)&&BigInt(s)<(1n<<256n);
const norm=s=>String(s??'').toLowerCase().replace(/[^a-z0-9]+/g,' ').trim();
const arr=v=>typeof v==='string'?JSON.parse(v):v;
const current=v=>v?.active===true&&v.closed===false&&v.archived!==true;
const optionalEnabled=(v,k)=>!(k in v)||v[k]===true;

function race(event){
  const text=norm(`${event.slug} ${event.title}`);
  let office=/\bhouse\b/.test(text)?'House':/\bsenate\b/.test(text)?'Senate':/\b(governor|gubernatorial)\b/.test(text)?'Governor':null;
  let state=STATE_ORDER.find(([,name])=>(' '+text+' ').includes(' '+norm(name)+' '))?.[0]??null;
  const house=String(event.slug??'').match(/(?:^|-)([a-z]{2})-(\d{1,2}|at-large)(?:-|$)/i);
  let district=null;
  if(office==='House'&&house&&STATES[house[1].toUpperCase()]){state=house[1].toUpperCase();district=house[2]==='at-large'?'0':String(Number(house[2]));}
  return {office,state,district,specialElection:/\bspecial\b/.test(text)};
}
function party(question){
  const s=norm(question);
  if(/^will (the )?(democrats|democratic party) win\b/.test(s))return 'Democratic';
  if(/^will (the )?(republicans|republican party) win\b/.test(s))return 'Republican';
  if(/^will (an )?(independent|independent party) win\b/.test(s))return 'Independent';
  return null;
}
function winnerQuestion(m){
  const q=String(m.question??'');
  return /^will .+ win .+\?$/i.test(q)&&!/\b(by|margin|primary|nomination|popular vote)\b/i.test(q)
    &&/\b2026\b/.test(q+' '+m.description)&&!(/\b20(?:2[0-57-9]|[3-9]\d)\b/.test(q));
}
function pairedAssets(m){
  const outcomes=arr(m.outcomes);need(Array.isArray(outcomes)&&outcomes.length===2&&outcomes.every(x=>typeof x==='string')&&new Set(outcomes.map(norm)).size===2&&outcomes.every(x=>['yes','no'].includes(norm(x))),'PM_OUTCOMES');
  const version=m.version??(m.positionIds!=null?'v2':'v1');need(['v1','v2'].includes(version),'PM_VERSION');
  const ids=version==='v2'?m.positionIds:arr(m.clobTokenIds);
  need(Array.isArray(ids)&&ids.length===outcomes.length&&ids.every(uint256)&&new Set(ids).size===2,'PM_ASSET_IDS');
  need(typeof m.conditionId==='string'&&/^0x[0-9a-fA-F]{64}$/.test(m.conditionId),'PM_CONDITION_ID');
  need(typeof m.id==='string'&&uint256(m.id),'PM_MARKET_ID');
  return outcomes.map((outcome,i)=>({assetId:ids[i],outcome,version}));
}
function receiptEvents(receipt){
  need(typeof receipt.bodyText==='string'&&Buffer.byteLength(receipt.bodyText)<=MAX_BODY&&sha(Buffer.from(receipt.bodyText))===receipt.bodySha256,'PM_RECEIPT_BODY_HASH');
  const endpoint=receipt.endpoint??receipt.path;
  const u=new URL(endpoint,'https://gamma-api.polymarket.com');
  need(u.origin==='https://gamma-api.polymarket.com'&&['/events','/public-search'].includes(u.pathname),'PM_RECEIPT_ENDPOINT');
  need(Number.isFinite(Date.parse(receipt.requestStartedAt))&&Date.parse(receipt.receivedAt)>=Date.parse(receipt.requestStartedAt),'PM_RECEIPT_CLOCK');
  if(receipt.status!==200)return {events:[],endpoint:u.href};
  let data;try{data=JSON.parse(receipt.bodyText);}catch{return {events:[],endpoint:u.href,parseError:true};}
  return {events:Array.isArray(data)?data:data?.events,endpoint:u.href,pagination:Array.isArray(data)?null:data?.pagination??null};
}

/** Pure replay of original public discovery responses, including failures. */
export function buildUniverse({receipts,inventory,joins,sources=[]}){
  need(Array.isArray(receipts)&&receipts.length<=256&&Array.isArray(inventory?.markets)&&Array.isArray(joins)&&inventory.markets.length<=1000&&joins.length<=1000,'PM_INPUT_BOUND');
  const inventoryIds=new Set(inventory.markets.map(m=>String(m.market_id)));need(inventoryIds.size===inventory.markets.length,'PM_INVENTORY_DUPLICATE');
  const identities=joins.filter(j=>inventoryIds.has(String(j.cup_market_id))&&j.cup_election_year===2026);
  const outcomes=new Map(),eventsSeen=new Set(),marketsSeen=new Set(),rejected=[];const metadata=[];
  for(const receipt of receipts){
    const decoded=receiptEvents(receipt);const receiptRef={endpoint:decoded.endpoint,requestStartedAt:receipt.requestStartedAt,receivedAt:receipt.receivedAt,status:receipt.status,bodySha256:receipt.bodySha256,receiptSha256:sha(JSON.stringify(receipt)),pagination:decoded.pagination??null};metadata.push(receiptRef);
    if(receipt.status!==200){rejected.push({endpoint:decoded.endpoint,reason:'HTTP_STATUS',status:receipt.status});continue;}
    if(decoded.parseError){rejected.push({endpoint:decoded.endpoint,reason:'INVALID_JSON_RESPONSE'});continue;}
    need(Array.isArray(decoded.events)&&decoded.events.length<=1000,'PM_EVENT_RESPONSE');
    for(const event of decoded.events){
      if(!current(event)){rejected.push({eventId:event.id??null,reason:'EVENT_INACTIVE_OR_CLOSED'});continue;}
      if(typeof event.slug!=='string'||!event.slug||typeof event.title!=='string'||!event.title){rejected.push({eventId:event.id??null,reason:'EVENT_IDENTITY'});continue;}
      need(Array.isArray(event.markets)&&event.markets.length<=1000,'PM_EVENT_MARKETS');eventsSeen.add(String(event.id));
      const scope=race(event);
      for(const m of event.markets){
        if(!current(m)||!optionalEnabled(m,'enableOrderBook')||!optionalEnabled(m,'acceptingOrders')){rejected.push({marketId:m.id??null,reason:'MARKET_NOT_CURRENT_ORDERBOOK'});continue;}
        if(!winnerQuestion(m)){rejected.push({marketId:m.id??null,reason:'NOT_2026_GENERAL_WINNER'});continue;}
        let pairs;try{pairs=pairedAssets(m);}catch(e){rejected.push({marketId:m.id??null,reason:e.message});continue;}
        const declaredParty=party(m.question);
        const raceRefs=identities.filter(j=>scope.state&&scope.office&&j.cup_office===scope.office&&j.cup_state_code===scope.state&&(scope.office!=='House'||(j.cup_district!==null&&j.cup_district!==undefined&&String(Number(j.cup_district))===scope.district)));
        const stable={eventId:String(event.id),eventSlug:event.slug,eventTitle:event.title,marketId:m.id,conditionId:m.conditionId,question:m.question,marketSlug:m.slug??null,rules:m.description??null,resolutionSource:m.resolutionSource??null,groupItemTitle:m.groupItemTitle??null,version:pairs[0].version,endDate:m.endDate??event.endDate??null};
        const metadataIdentitySha256=sha(JSON.stringify(stable));marketsSeen.add(m.id);
        for(const pair of pairs){
          const sigReferences=raceRefs.map(j=>({sigMarketId:String(j.cup_market_id),sigParty:j.cup_party,sigOffice:j.cup_office,sigState:j.cup_state_code,sigDistrict:j.cup_district??null,sigElectionYear:2026,
            relationship:declaredParty===j.cup_party?'same_party_conditional':declaredParty?'race_context_only':'candidate_race_context',sigSide:declaredParty===j.cup_party?(norm(pair.outcome)==='yes'?'YES':'NO'):null,
            specialStageVerified:j.cup_special_election!==null&&j.cup_special_election!==undefined&&j.cup_special_election===scope.specialElection,sigIdentityObservedAt:j.cup_identity_observed_at??null,pricingAuthority:false,resolutionEquivalenceVerified:false}));
          const row={...stable,...pair,metadataIdentitySha256,scope,publisherPartyQuestion:declaredParty,mappingKind:declaredParty?'conditional_party_question':'candidate_race_context',sigReferences,pricingAuthority:false,resolutionEquivalenceVerified:false,conditional_resolution_unverified:true,sourceReceipts:[receiptRef]};
          const previous=outcomes.get(pair.assetId);
          if(previous){need(previous.metadataIdentitySha256===metadataIdentitySha256&&previous.outcome===pair.outcome,'PM_ASSET_IDENTITY_CONFLICT');if(!previous.sourceReceipts.some(r=>r.receiptSha256===receiptRef.receiptSha256))previous.sourceReceipts.push(receiptRef);}
          else{need(outcomes.size<MAX_OUTCOMES,'PM_OUTCOME_LIMIT');outcomes.set(pair.assetId,row);}
        }
      }
    }
  }
  const rows=[...outcomes.values()].sort((a,b)=>a.eventSlug.localeCompare(b.eventSlug)||a.marketId.localeCompare(b.marketId)||a.outcome.localeCompare(b.outcome));
  const referenced=new Set(rows.flatMap(r=>r.sigReferences.map(s=>s.sigMarketId))),direct=new Set(rows.flatMap(r=>r.sigReferences.filter(s=>s.relationship==='same_party_conditional').map(s=>s.sigMarketId)));
  return {schemaVersion:1,source:'polymarket_gamma_public',pricingAuthority:false,resolutionEquivalenceVerified:false,outcomes:rows,metadata,sources,rejected,
    unmatchedSigMarketIds:[...inventoryIds].filter(id=>!referenced.has(id)).sort(),summary:{sigInventoryMarkets:inventoryIds.size,observedActiveEvents:eventsSeen.size,observedWinnerMarkets:marketsSeen.size,uniqueOutcomeAssets:rows.length,raceReferencedSigMarkets:referenced.size,samePartyConditionalSigMarkets:direct.size,unmatchedSigMarkets:inventoryIds.size-referenced.size,discoveryResponses:receipts.length,truncatedSearchQueries:metadata.filter(r=>r.pagination?.hasMore===true).length,httpErrors:rejected.filter(r=>r.reason==='HTTP_STATUS').length,rejectedRows:rejected.length,notClaimed:'No settlement equivalence, current SIG liquidity, exhaustive catalog coverage or calibrated probabilities'}};
}

export function loadInventory(repositoryRoot){
  const base='research/crossvenue_screen_2026-10-02/';const sources=[];
  const read=name=>{const relative=base+name,b=readFileSync(resolve(repositoryRoot,relative));need(b.length<=32*1024*1024,'PM_INVENTORY_COMPRESSED_BOUND');const raw=gunzipSync(b,{maxOutputLength:64*1024*1024});sources.push({path:relative,sha256:sha(b),decodedSha256:sha(raw)});return JSON.parse(raw);};
  return {inventory:read('public_market_inventory_2026-10-01_184851.json.gz'),joins:read('join_records.json.gz'),sources};
}
/** These are DISCOVERY queries, never constructed market/token identities. */
export function discoveryPaths(joins){
  const paths=new Set();
  for(const j of joins.filter(j=>j.cup_election_year===2026)){
    const state=STATES[j.cup_state_code];if(!state)continue;
    if(['Senate','Governor'].includes(j.cup_office))paths.add('/events?slug='+state.toLowerCase().replaceAll(' ','-')+'-'+j.cup_office.toLowerCase()+'-election-winner');
    else if(j.cup_office==='House')paths.add('/public-search?'+new URLSearchParams({q:`${j.cup_state_code}-${String(Number(j.cup_district)).padStart(2,'0')} House Election Winner 2026`,limit_per_type:'10',events_status:'active'}));
  }
  for(const q of ['2026 House control','2026 Senate control'])paths.add('/public-search?'+new URLSearchParams({q,limit_per_type:'10',events_status:'active'}));
  return [...paths];
}
async function publicReceipt(path){
  const endpoint='https://gamma-api.polymarket.com'+path,requestStartedAt=new Date().toISOString();
  const response=await fetch(endpoint,{signal:AbortSignal.timeout(20_000),headers:{Accept:'application/json'}});
  const chunks=[];let size=0;
  for await(const chunk of response.body){size+=chunk.length;need(size<=MAX_BODY,'PM_PUBLIC_BODY_BOUND');chunks.push(chunk);}
  const raw=Buffer.concat(chunks),bodyText=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(raw);
  return {source:'polymarket_gamma_public',endpoint,requestStartedAt,receivedAt:new Date().toISOString(),status:response.status,bodyText,bodySha256:sha(raw),responseHeaders:{date:response.headers.get('date'),retryAfter:response.headers.get('retry-after')},sourceTimestamp:null};
}
function immutable(path,bytes){if(existsSync(path)){need(readFileSync(path).equals(bytes),'PM_CACHE_IMMUTABLE_CONFLICT');return;}writeFileSync(path,bytes,{flag:'wx',mode:0o600});}

/** Explicit one-off public discovery, at most five GET starts/sec. Cached replay
 * uses original clocks and must be explicitly requested, never timestamp-renewed. */
export async function loadUniverse({repositoryRoot=resolve(dirname(fileURLToPath(import.meta.url)),'../..'),cacheDirectory,maximumRequests=200,fetchReceipt=publicReceipt,offline=false}={}){
  need(typeof cacheDirectory==='string'&&cacheDirectory.length>0,'PM_CACHE_REQUIRED');need(Number.isInteger(maximumRequests)&&maximumRequests>0&&maximumRequests<=256,'PM_REQUEST_BOUND');
  const inputs=loadInventory(repositoryRoot);mkdirSync(cacheDirectory,{recursive:true,mode:0o700});
  let receipts=[],failures=[],snapshot;
  if(offline){snapshot=JSON.parse(readFileSync(resolve(cacheDirectory,'snapshot.json'),'utf8'));need(JSON.stringify(snapshot.inventorySources)===JSON.stringify(inputs.sources),'PM_CACHE_INVENTORY_CHANGED');need(Array.isArray(snapshot.receipts)&&snapshot.receipts.length<=256,'PM_SNAPSHOT_BOUND');for(const r of snapshot.receipts){need(/^[a-f0-9]{64}\.receipt\.json$/.test(r.file),'PM_CACHE_FILE');const b=readFileSync(resolve(cacheDirectory,r.file));need(b.length<=2*MAX_BODY&&sha(b)===r.sha256,'PM_CACHE_RECEIPT_SHA');receipts.push(JSON.parse(b));}failures=snapshot.failures??[];}
  else{
    const paths=discoveryPaths(inputs.joins);let lastStart=-Infinity;
    for(const path of paths.slice(0,maximumRequests)){
      const wait=Math.max(0,200-(performance.now()-lastStart));if(wait)await new Promise(r=>setTimeout(r,wait));lastStart=performance.now();
      try{const receipt=await fetchReceipt(path);receiptEvents(receipt);receipts.push(receipt);if(receipt.status===429){failures.push({path,reason:'RATE_LIMIT_STOP',retryAfter:receipt.responseHeaders?.retryAfter??null});break;}}
      catch(error){failures.push({path,reason:String(error.message),failedAt:new Date().toISOString()});}
    }
    const pins=receipts.map(r=>{const bytes=Buffer.from(JSON.stringify(r)),hash=sha(bytes),file=hash+'.receipt.json';immutable(resolve(cacheDirectory,file),bytes);immutable(resolve(cacheDirectory,r.bodySha256+'.body'),Buffer.from(r.bodyText));return {file,sha256:hash};});
    snapshot={schemaVersion:1,source:'polymarket_gamma_public',inventorySources:inputs.sources,receipts:pins,failures,plannedQueries:paths.length,attemptedBound:maximumRequests,plannedQueryPassComplete:failures.length===0&&maximumRequests>=paths.length,catalogExhaustive:false};
    const bytes=Buffer.from(JSON.stringify(snapshot,null,2));immutable(resolve(cacheDirectory,sha(bytes)+'.snapshot.json'),bytes);const temp=resolve(cacheDirectory,`snapshot.${process.pid}.tmp`);writeFileSync(temp,bytes,{flag:'wx',mode:0o600});renameSync(temp,resolve(cacheDirectory,'snapshot.json'));
  }
  const universe=buildUniverse({...inputs,receipts});return {...universe,discovery:{...snapshot,discoveryComplete:snapshot.plannedQueryPassComplete&&universe.summary.truncatedSearchQueries===0,cacheDirectory:resolve(cacheDirectory),offlineReplay:offline}};
}

if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const args=process.argv.slice(2),options={};for(let i=0;i<args.length;i++){if(args[i]==='--offline'){options.offline=true;continue;}need(['--output','--repository-root','--cache','--maximum-requests'].includes(args[i])&&i+1<args.length,'PM_CLI_ARGUMENT');options[args[i].slice(2)]=args[++i];}
  need(options.output,'PM_OUTPUT_REQUIRED');const output=resolve(options.output);
  const universe=await loadUniverse({repositoryRoot:options['repository-root'],cacheDirectory:options.cache??output+'.cache',maximumRequests:options['maximum-requests']===undefined?200:Number(options['maximum-requests']),offline:options.offline??false});
  mkdirSync(dirname(output),{recursive:true,mode:0o700});const temp=output+`.${process.pid}.tmp`;writeFileSync(temp,JSON.stringify(universe,null,2),{flag:'wx',mode:0o600});renameSync(temp,output);console.log(JSON.stringify({output,summary:universe.summary,discoveryComplete:universe.discovery.discoveryComplete,failures:universe.discovery.failures}));
}
