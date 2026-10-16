// Offline discovery only. A catalog identity is not a settlement-equivalence proof.
import { readFileSync, existsSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
/** Exact metadata identity; this excludes live price/volume so ordinary trading
 * updates do not change the reviewed contract text. Missing fields hash null. */
export function metadataIdentitySha256(market) {
  return sha(JSON.stringify(['ticker','event_ticker','title','subtitle','rules_primary','rules_secondary'].map(key=>market?.[key]??null)));
}
const sorted = values => [...new Set(values)].sort((a,b)=>a.localeCompare(b,'en',{numeric:true}));
const party = value => ({Democratic:'DEM',Republican:'REP',Independent:'IND'}[value] ?? null);
const district = value => value == null ? null : String(value).replace(/^0+(?=\d)/,'');
// Actual returned election events can carry a month suffix (26NOV). Their
// explicit catalog election_year must ALSO be2026; no ticker is constructed.
const tickerValid = value => typeof value === 'string' && /^[A-Z0-9][A-Z0-9_-]{0,127}$/.test(value) && /(?:^|-)26(?:JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC)?(?:-|$)/.test(value) && !/(?:^|-)28(?:[A-Z]*)(?:-|$)/.test(value);
const marketId = value => /^\d+$/.test(String(value)) ? String(value) : null;
const sameRace = (join,ref) => join.cup_election_year === 2026 && ref.election_year === 2026 &&
  join.cup_office === ref.office && join.cup_state_code === ref.state_code &&
  district(join.cup_district) === district(ref.district) &&
  party(join.cup_party) !== null && party(join.cup_party) === party(ref.party) &&
  (join.cup_special_election == null || join.cup_special_election === ref.special_election);

/** Pure bounded catalog join. No HTTP, credentials, pricing inference or authority. */
export function buildUniverse({catalog,leads,joins,inventory,sources=[],maximumTickers=1024}) {
  if (!Number.isInteger(maximumTickers) || maximumTickers<1 || maximumTickers>2048) throw Error('UNIVERSE_BOUND');
  if (!Array.isArray(catalog?.markets) || !Array.isArray(leads) || !Array.isArray(joins) || !Array.isArray(inventory?.markets)) throw Error('UNIVERSE_SCHEMA');
  if ([catalog.markets,leads,joins,inventory.markets].some(rows=>rows.length>10000)) throw Error('UNIVERSE_INPUT_BOUND');
  const ids = sorted(inventory.markets.map(row=>marketId(row.market_id)).filter(Boolean));
  if (ids.length!==inventory.markets.length) throw Error('UNIVERSE_INVENTORY_ID');
  const allowed = new Set(ids), markets = new Map(), rejected=[];
  const references = new Map();
  for (const ref of catalog.markets) {
    if (ref.election_year!==2026 || !tickerValid(ref.ticker)) { rejected.push({ticker:ref.ticker??null,reason:'NON_2026_OR_INVALID_TICKER'}); continue; }
    const previous=references.get(ref.ticker);
    if (previous && JSON.stringify([previous.race_key,previous.party,previous.office,previous.state_code,previous.district])!==JSON.stringify([ref.race_key,ref.party,ref.office,ref.state_code,ref.district])) throw Error('UNIVERSE_CONFLICTING_TICKER');
    references.set(ref.ticker,ref);
  }
  const add=(ref,join,origin,side='YES')=>{
    const id=marketId(join.cup_market_id);
    if (!id || !allowed.has(id) || join.cup_election_year!==2026 || !['YES','NO'].includes(side)) return;
    let row=markets.get(ref.ticker);
    if (!row) {
      row={ticker:ref.ticker,eventTicker:ref.event_ticker??null,electionYear:2026,office:ref.office??null,state:ref.state_code??null,district:ref.district??null,party:ref.party??null,raceKey:ref.race_key??null,historicalStatus:ref.status??null,currentStatus:'unverified',referenceSourceUrl:ref.market_details_api_url??ref.source_url??null,referenceObservedAt:ref.quote_receipt_utc??null,referenceRulesPrimary:ref.rules_primary??null,conditional_resolution_unverified:true,sigMappings:[]};
      markets.set(ref.ticker,row);
    }
    const mapping={sigMarketId:id,sigParty:join.cup_party??null,sigOffice:join.cup_office??null,sigState:join.cup_state_code??null,sigDistrict:join.cup_district??null,sigElectionYear:2026,side,origin,sigObservedAt:join.cup_identity_observed_at??null,sigIdentityProvenance:join.cup_identity_provenance??[],semanticFlags:join.semantic_and_data_flags??[]};
    if (!row.sigMappings.some(m=>JSON.stringify(m)===JSON.stringify(mapping))) row.sigMappings.push(mapping);
  };
  for (const lead of leads) {
    const ref=references.get(lead.reference_ticker);
    if (!ref || lead.reference_election_year!==2026) continue;
    // Preserve a recorded conditional mapping even where the original comparison
    // used the NO side; do not silently reinterpret it as same-party YES.
    add(ref,lead,'recorded_conditional_lead',lead.side);
  }
  for (const join of joins) for (const ref of references.values()) if (sameRace(join,ref)) add(ref,join,'structured_catalog_identity');
  const rank=(a,b)=>{
    const ar=references.get(a.ticker),br=references.get(b.ticker);
    return Number(!ar.party_rule_verified)-Number(!br.party_rule_verified)||Number(!ar.full_orderbook_file)-Number(!br.full_orderbook_file)||
      (Date.parse(br.quote_receipt_utc)||0)-(Date.parse(ar.quote_receipt_utc)||0)||a.ticker.localeCompare(b.ticker);
  };
  const selected=new Map(),alternates=[];
  for (const id of ids) {
    // ONLY the same-party structured identity can nominate a poll. A historical
    // opposite-party NO comparison does not establish logical equivalence.
    const candidates=[...markets.values()].filter(row=>row.sigMappings.some(m=>m.sigMarketId===id&&m.origin==='structured_catalog_identity'&&m.side==='YES'&&party(m.sigParty)===party(row.party))).sort(rank);
    if (!candidates.length) continue;
    const primary=candidates[0];let row=selected.get(primary.ticker);
    if (!row) {row={...primary,sigMappings:[]};selected.set(row.ticker,row);}
    row.sigMappings.push(...primary.sigMappings.filter(m=>m.sigMarketId===id&&m.origin==='structured_catalog_identity'));
    alternates.push({sigMarketId:id,primaryTicker:primary.ticker,alternateTickers:candidates.slice(1).map(m=>m.ticker),conditionalLeadTickers:sorted(leads.filter(l=>String(l.cup_market_id)===id&&references.has(l.reference_ticker)).map(l=>l.reference_ticker))});
  }
  const rows=[...selected.values()].sort((a,b)=>a.ticker.localeCompare(b.ticker));
  if (rows.length>maximumTickers) throw Error('UNIVERSE_TICKER_BOUND');
  for (const row of rows) {row.sigMappings.sort((a,b)=>a.sigMarketId.localeCompare(b.sigMarketId,'en',{numeric:true}));row.sigMarketIds=sorted(row.sigMappings.map(m=>m.sigMarketId));}
  const mapped=new Set(rows.flatMap(row=>row.sigMarketIds));
  return {schemaVersion:1,scope:'retained_237_SIG_catalog_not_entire_Kalshi_platform',conditional_resolution_unverified:true,settlementEquivalenceVerified:false,sources,sourceInventoryObservedAt:inventory.captured_at??null,referenceCatalogObservedAt:catalog.metadata?.created_utc??null,sigMarketIds:ids,unmappedSigMarketIds:ids.filter(id=>!mapped.has(id)),markets:rows,alternates,summary:{sigMarkets:ids.length,mappedSigMarkets:mapped.size,unmappedSigMarkets:ids.length-mapped.size,uniqueTickers:rows.length,idealSweepSecondsAt20ReadsPerSecond:rows.length/20,recordedLeadRows:leads.length,rejectedReferenceRows:rejected.length},rejected};
}

/** Add only previously unmapped identities backed by original public receipts.
 * The special2026 whitelist is exact metadata, never a ticker suffix override. */
export function extendUniverse(base,supplement,readReceipt) {
  if (supplement?.schemaVersion!==1||supplement.conditional_resolution_unverified!==true||!Array.isArray(supplement.mappings)||supplement.mappings.length>32) throw Error('UNIVERSE_SUPPLEMENT_SCHEMA');
  const out=structuredClone(base),eligible=new Set(base.unmappedSigMarketIds),seen=new Set();
  const pointer=(v,p)=>p.split('/').slice(1).reduce((a,k)=>a?.[k],v);
  for(const mapping of supplement.mappings){
    if (!eligible.has(mapping.sigMarketId)||seen.has(mapping.sigMarketId)||mapping.electionYear!==2026||!party(mapping.party)) throw Error('UNIVERSE_SUPPLEMENT_IDENTITY');
    if(!/^research\/moloch-kalshi-mapping-20261016\/receipts\/[A-Za-z0-9_-]+\.receipt\.json\.gz$/.test(mapping.receiptPath)||!/^\/body\/events\/\d+\/markets\/\d+$/.test(mapping.marketPointer)||mapping.eventPointer!==mapping.marketPointer.replace(/\/markets\/\d+$/,''))throw Error('UNIVERSE_SUPPLEMENT_SOURCE_PATH');
    const {receipt,raw,compressed}=readReceipt(mapping.receiptPath);
    if(sha(raw)!==mapping.receiptSha256||sha(compressed)!==mapping.compressedSha256||receipt.status!==200||sha(receipt.bodyText)!==receipt.bodySha256||receipt.bodySha256!==mapping.bodySha256||JSON.stringify(JSON.parse(receipt.bodyText))!==JSON.stringify(receipt.body))throw Error('UNIVERSE_SUPPLEMENT_SOURCE_HASH');
    if(receipt.receivedAt!==mapping.receivedAt||receipt.requestStartedAt!==mapping.requestStartedAt||!Number.isFinite(Date.parse(receipt.receivedAt))||!Number.isFinite(Date.parse(receipt.requestStartedAt))||Date.parse(receipt.requestStartedAt)>Date.parse(receipt.receivedAt)||!receipt.path.startsWith('/trade-api/v2/events?'))throw Error('UNIVERSE_SUPPLEMENT_SOURCE_CLOCK');
    const m=pointer(receipt,mapping.marketPointer),e=pointer(receipt,mapping.eventPointer);
    if(!m||m.ticker!==mapping.ticker||e?.event_ticker!==mapping.eventTicker||m.event_ticker!==mapping.eventTicker||m.rules_primary!==mapping.rulesPrimary||!/\b2026\b/.test(m.rules_primary)||/\b2028\b/.test(m.rules_primary)||!new RegExp(mapping.party+' (?:party|Party)','i').test(m.rules_primary))throw Error('UNIVERSE_SUPPLEMENT_RULE');
    const identity=metadataIdentitySha256(m);
    if(mapping.metadataIdentitySha256!==identity||mapping.verifiedElectionYear!==2026)throw Error('UNIVERSE_SUPPLEMENT_METADATA');
    let row=out.markets.find(v=>v.ticker===m.ticker);
    if(row&&row.metadataIdentitySha256&&row.metadataIdentitySha256!==identity)throw Error('UNIVERSE_SUPPLEMENT_CONFLICT');
    const sigMapping={sigMarketId:mapping.sigMarketId,sigParty:mapping.party,sigOffice:mapping.office,sigState:mapping.state,sigDistrict:mapping.district,sigElectionYear:2026,side:'YES',origin:'original_current_metadata_supplement',sigObservedAt:receipt.receivedAt,sigIdentityProvenance:[{receiptPath:mapping.receiptPath,receiptSha256:mapping.receiptSha256,marketPointer:mapping.marketPointer}],semanticFlags:['conditional_resolution_unverified']};
    if(!row){row={ticker:m.ticker,eventTicker:m.event_ticker,electionYear:2026,verifiedElectionYear:2026,metadataIdentitySha256:identity,office:mapping.office,state:mapping.state,district:mapping.district,party:mapping.party,historicalStatus:m.status??null,currentStatus:'unverified_after_retained_receipt',referenceObservedAt:receipt.receivedAt,referenceSourceUrl:receipt.path,referenceRulesPrimary:m.rules_primary,conditional_resolution_unverified:true,sigMappings:[],sigMarketIds:[]};out.markets.push(row);}
    row.sigMappings.push(sigMapping);row.sigMarketIds=sorted([...row.sigMarketIds,mapping.sigMarketId]);seen.add(mapping.sigMarketId);
  }
  out.markets.sort((a,b)=>a.ticker.localeCompare(b.ticker));out.unmappedSigMarketIds=base.unmappedSigMarketIds.filter(id=>!seen.has(id));out.supplementCloseMatches=supplement.closeMatches??[];
  if(!Array.isArray(out.supplementCloseMatches)||out.supplementCloseMatches.length>32)throw Error('UNIVERSE_ANALOG_BOUND');
  out.candidateAnalogMarkets=[];const analogIds=new Set();
  for(const a of out.supplementCloseMatches){
    if(!out.unmappedSigMarketIds.includes(a.sigMarketId)||analogIds.has(a.sigMarketId)||!a.ruleMismatch||a.verifiedElectionYear!==2026||!/^research\/moloch-kalshi-mapping-20261016\/receipts\/[A-Za-z0-9_-]+\.receipt\.json\.gz$/.test(a.receiptPath)||!/^\/body\/events\/\d+\/markets\/\d+$/.test(a.marketPointer)||a.eventPointer!==a.marketPointer.replace(/\/markets\/\d+$/,''))throw Error('UNIVERSE_ANALOG_IDENTITY');
    const {receipt,raw,compressed}=readReceipt(a.receiptPath);
    if(sha(raw)!==a.receiptSha256||sha(compressed)!==a.compressedSha256||receipt.status!==200||sha(receipt.bodyText)!==receipt.bodySha256||receipt.bodySha256!==a.bodySha256||JSON.stringify(JSON.parse(receipt.bodyText))!==JSON.stringify(receipt.body))throw Error('UNIVERSE_ANALOG_SOURCE_HASH');
    if(receipt.receivedAt!==a.receivedAt||receipt.requestStartedAt!==a.requestStartedAt||!Number.isFinite(Date.parse(receipt.receivedAt))||!Number.isFinite(Date.parse(receipt.requestStartedAt))||Date.parse(receipt.requestStartedAt)>Date.parse(receipt.receivedAt)||!receipt.path.startsWith('/trade-api/v2/events?'))throw Error('UNIVERSE_ANALOG_CLOCK');
    const m=pointer(receipt,a.marketPointer),e=pointer(receipt,a.eventPointer);
    if(!m||m.ticker!==a.ticker||m.event_ticker!==a.eventTicker||e?.event_ticker!==a.eventTicker||e.sub_title!=='In 2026'||m.rules_primary!==a.rulesPrimary||/\b2028\b/.test(m.rules_primary)||metadataIdentitySha256(m)!==a.metadataIdentitySha256||out.markets.some(v=>v.ticker===a.ticker))throw Error('UNIVERSE_ANALOG_METADATA');
    out.candidateAnalogMarkets.push({ticker:a.ticker,eventTicker:a.eventTicker,sigMarketId:a.sigMarketId,sigMarketIds:[a.sigMarketId],candidateName:a.candidateName,verifiedElectionYear:2026,metadataIdentitySha256:a.metadataIdentitySha256,mappingKind:'candidate_reference_analog',resolutionEquivalenceVerified:false,pricingAuthority:false,conditional_resolution_unverified:true,ruleMismatch:a.ruleMismatch,rulesPrimary:m.rules_primary,referenceObservedAt:receipt.receivedAt,sourceEvidence:{receiptPath:a.receiptPath,receiptSha256:a.receiptSha256,marketPointer:a.marketPointer}});analogIds.add(a.sigMarketId);
  }
  out.summary={...out.summary,mappedSigMarkets:base.summary.mappedSigMarkets+seen.size,directMappedSigMarkets:base.summary.mappedSigMarkets+seen.size,analogCoveredSigMarkets:analogIds.size,totalReferenceCoveredSigMarkets:base.summary.mappedSigMarkets+seen.size+analogIds.size,unmappedSigMarkets:out.unmappedSigMarketIds.length,uniqueTickers:out.markets.length,uniqueCandidateAnalogTickers:new Set(out.candidateAnalogMarkets.map(a=>a.ticker)).size,idealSweepSecondsAt20ReadsPerSecond:out.markets.length/20,idealCombinedReferenceSweepSecondsAt20ReadsPerSecond:(new Set([...out.markets,...out.candidateAnalogMarkets].map(v=>v.ticker))).size/20};return out;
}

/** Load original repository gzip bytes, preserving their hashes and clocks. */
export function loadUniverse({repositoryRoot=resolve(dirname(fileURLToPath(import.meta.url)),'../..'),maximumTickers=1024}={}) {
  const sources=[];
  const read=relative=>{
    const bytes=readFileSync(resolve(repositoryRoot,relative));
    if (bytes.length>32*1024*1024) throw Error('UNIVERSE_COMPRESSED_BOUND');
    const raw=gunzipSync(bytes,{maxOutputLength:64*1024*1024});
    sources.push({path:relative,sha256:sha(bytes),decodedSha256:sha(raw)});
    return JSON.parse(raw.toString('utf8'));
  };
  const base='research/crossvenue_screen_2026-10-02/';
  const catalog=read('research/public_reference_2026-10-02/reference.json.gz');
  const leads=read(base+'conditional_crossvenue_leads.json.gz');
  const joins=read(base+'join_records.json.gz');
  const inventory=read(base+'public_market_inventory_2026-10-01_184851.json.gz');
  const baseUniverse=buildUniverse({catalog,leads,joins,inventory,sources,maximumTickers});
  const supplementPath=resolve(repositoryRoot,'research/moloch-kalshi-mapping-20261016/supplement.json');
  if(!existsSync(supplementPath))return baseUniverse;
  const result=extendUniverse(baseUniverse,JSON.parse(readFileSync(supplementPath,'utf8')),relative=>{const compressed=readFileSync(resolve(repositoryRoot,relative));if(compressed.length>4*1024*1024)throw Error('UNIVERSE_SUPPLEMENT_BOUND');const raw=gunzipSync(compressed,{maxOutputLength:16*1024*1024});return{compressed,raw,receipt:JSON.parse(raw.toString('utf8'))};});
  if(result.markets.length>maximumTickers)throw Error('UNIVERSE_TICKER_BOUND');return result;
}

if (process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  console.log(JSON.stringify(loadUniverse({repositoryRoot:process.argv[2]??undefined}),null,2));
}
