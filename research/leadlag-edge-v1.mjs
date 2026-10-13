import {createHash} from 'node:crypto';
import {instantNs} from '../feed/contract.mjs';

const STATES = [
  'Alabama','Alaska','Arizona','Arkansas','California','Colorado','Connecticut','Delaware','Florida','Georgia',
  'Hawaii','Idaho','Illinois','Indiana','Iowa','Kansas','Kentucky','Louisiana','Maine','Maryland','Massachusetts',
  'Michigan','Minnesota','Mississippi','Missouri','Montana','Nebraska','Nevada','New Hampshire','New Jersey',
  'New Mexico','New York','North Carolina','North Dakota','Ohio','Oklahoma','Oregon','Pennsylvania','Rhode Island',
  'South Carolina','South Dakota','Tennessee','Texas','Utah','Vermont','Virginia','Washington','West Virginia',
  'Wisconsin','Wyoming','District of Columbia'
];

const isoMs = (value, code) => {
  const ms = Date.parse(value);
  if (!Number.isFinite(ms) || new Date(ms).toISOString() !== value) throw Error(code);
  return ms;
};

function raceIdentity(raceName) {
  if (typeof raceName !== 'string') throw Error('LEADLAG_RACE_NAME');
  let type, remainder;
  if (raceName.startsWith('Governor of ')) { type = 'governor'; remainder = raceName.slice('Governor of '.length); }
  else if (raceName.startsWith('U.S. Senate ')) { type = 'senate'; remainder = raceName.slice('U.S. Senate '.length); }
  else if (raceName.startsWith('U.S. House ')) { type = 'house'; remainder = raceName.slice('U.S. House '.length).replace(/ District \d+$/, ''); }
  else throw Error('LEADLAG_RACE_TYPE');
  if (!STATES.includes(remainder)) throw Error('LEADLAG_STATE_GROUP');
  return {type, stateGroup: `state:${remainder}`};
}

// Build a conservative graph from captured, source-backed race roots. Repeated
// pair rows (e.g. a three-party race) are unioned by raceId before use.
export function leadLagMarketGraphV1(structuralUniverse) {
  if (!structuralUniverse || !Array.isArray(structuralUniverse.candidatePairs)) throw Error('LEADLAG_METADATA');
  const races = new Map();
  for (const pair of structuralUniverse.candidatePairs) {
    if (typeof pair.raceId !== 'string' || !Array.isArray(pair.legs) || pair.legs.length !== 2) throw Error('LEADLAG_PAIR');
    const identity = raceIdentity(pair.raceName);
    let race = races.get(pair.raceId);
    if (!race) races.set(pair.raceId, race = {raceId: pair.raceId, raceName: pair.raceName, ...identity, members: new Map()});
    if (race.raceName !== pair.raceName || race.stateGroup !== identity.stateGroup || race.type !== identity.type) throw Error('LEADLAG_RACE_CONFLICT');
    for (const leg of pair.legs) {
      const marketId = String(leg.marketId), exchangeId = String(leg.exchangeId);
      if (!marketId || !exchangeId || typeof leg.party !== 'string' || !leg.party) throw Error('LEADLAG_MEMBER');
      const prior = race.members.get(exchangeId);
      if (prior && (prior.marketId !== marketId || prior.party !== leg.party)) throw Error('LEADLAG_MEMBER_CONFLICT');
      race.members.set(exchangeId, {marketId, exchangeId, party: leg.party});
    }
  }
  const markets = new Map(), exchanges = new Map();
  for (const race of races.values()) {
    const parties = new Set();
    for (const member of race.members.values()) {
      if (parties.has(member.party)) throw Error('LEADLAG_DUPLICATE_PARTY');
      parties.add(member.party);
      const record = {...member, raceId: race.raceId, raceName: race.raceName, stateGroup: race.stateGroup, type: race.type};
      const oldMarket = markets.get(record.marketId), oldExchange = exchanges.get(record.exchangeId);
      if (oldMarket && (oldMarket.raceId !== record.raceId || oldMarket.exchangeId !== record.exchangeId)) throw Error('LEADLAG_MARKET_CONFLICT');
      if (oldExchange && (oldExchange.marketId !== record.marketId || oldExchange.raceId !== record.raceId)) throw Error('LEADLAG_EXCHANGE_CONFLICT');
      markets.set(record.marketId, record); exchanges.set(record.exchangeId, record);
    }
  }
  return {markets, exchanges, raceCount: races.size, stateGroups: [...new Set([...races.values()].map(x => x.stateGroup))].sort()};
}

function heldOut(stateGroup, {heldoutStateGroups, holdoutModulo, holdoutRemainder}) {
  if (heldoutStateGroups) return heldoutStateGroups.has(stateGroup);
  const n = createHash('sha256').update(stateGroup).digest().readUInt32BE(0) % holdoutModulo;
  return n === holdoutRemainder;
}

function median(values) {
  if (!values.length) return null;
  const sorted = values.slice().sort((a, b) => a - b), mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}
function upperBound(rows, at, strict = false) {
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if (strict ? rows[m].availableMs <= at : rows[m].availableMs < at) lo = m + 1;
    else hi = m;
  }
  return lo;
}
function hasBreakBetween(rows, start, end) {
  if (!rows?.length) return false;
  let lo = 0, hi = rows.length;
  while (lo < hi) { const mid = (lo + hi) >> 1; if (rows[mid] <= start) lo = mid + 1; else hi = mid; }
  return lo < rows.length && rows[lo] <= end;
}
function stale(row, at, maxDataAgeMs) {
  return row.sourceMs > at || at - row.sourceMs > maxDataAgeMs || row.expiryMs !== null && row.expiryMs <= at;
}

function validateOptions(options) {
  const positive = (xs, name, allowZero = false) => {
    if (!Array.isArray(xs) || !xs.length || xs.some(x => !Number.isSafeInteger(x) || x < (allowZero ? 0 : 1))) throw Error(`LEADLAG_${name}`);
    return [...new Set(xs)].sort((a, b) => a - b);
  };
  const lookbacksMs = positive(options.lookbacksMs ?? [1000, 5000, 30000], 'LOOKBACKS');
  const latenciesMs = positive(options.latenciesMs ?? [0, 250, 1000, 3000], 'LATENCIES', true);
  const horizonsMs = positive(options.horizonsMs ?? [1000, 5000, 30000], 'HORIZONS');
  const splitAtMs = isoMs(options.splitAt, 'LEADLAG_SPLIT');
  const defaults = {
    lookbacksMs, latenciesMs, horizonsMs, splitAtMs,
    maxPeerAgeMs: options.maxPeerAgeMs ?? 20000,
    maxDataAgeMs: options.maxDataAgeMs ?? 5000,
    maxEntryDelayMs: options.maxEntryDelayMs ?? 10000,
    maxExitDelayMs: options.maxExitDelayMs ?? 15000,
    minDecisionSpacingMs: options.minDecisionSpacingMs ?? 60000,
    minDepth: options.minDepth ?? 1,
    minTrainingLabels: options.minTrainingLabels ?? 100,
    holdoutModulo: options.holdoutModulo ?? 5,
    holdoutRemainder: options.holdoutRemainder ?? 0,
    heldoutStateGroups: options.heldoutStateGroups ? new Set(options.heldoutStateGroups) : null,
  };
  for (const k of ['maxPeerAgeMs','maxDataAgeMs','maxEntryDelayMs','maxExitDelayMs','minDecisionSpacingMs','minDepth','minTrainingLabels']) {
    if (!Number.isSafeInteger(defaults[k]) || defaults[k] < 0 || k === 'minDepth' && defaults[k] < 1) throw Error(`LEADLAG_${k.toUpperCase()}`);
  }
  if (!Number.isSafeInteger(defaults.holdoutModulo) || defaults.holdoutModulo < 2 || !Number.isSafeInteger(defaults.holdoutRemainder) || defaults.holdoutRemainder < 0 || defaults.holdoutRemainder >= defaults.holdoutModulo) throw Error('LEADLAG_HOLDOUT');
  return defaults;
}

// Offline, one-share quote replay. All feature states are read strictly before
// a timestamp group; fills use the next strictly later valid observation.
export function analyzeLeadLagV1({observations, breaks = [], graph, options}) {
  if (!Array.isArray(observations) || !Array.isArray(breaks) || !graph?.markets || !graph?.exchanges) throw Error('LEADLAG_INPUT');
  const cfg = validateOptions(options ?? {}), sourceSeries = new Map(), breaksByMarket = new Map(), breaksByAt = new Map(), diagnostics = {
    inputPoints: observations.length, acceptedPoints: 0, invalidPoints: 0, missingMetadata: 0,
    missingPeerFeature: 0, sampledDecisions: 0, censoredEntry: 0, censoredExit: 0,
    censoredBreak: 0, censoredDepth: 0, censoredStale: 0,
  };
  let prevAt = -Infinity;
  for (const b of breaks) {
    const at = isoMs(b.at, 'LEADLAG_BREAK_CLOCK');
    if (at < prevAt || b.marketId !== null && typeof b.marketId !== 'string') throw Error('LEADLAG_BREAK_ORDER');
    prevAt = at;
    const list = breaksByMarket.get(b.marketId) ?? [];
    list.push(at); breaksByMarket.set(b.marketId, list);
    const atList = breaksByAt.get(at) ?? new Set(); atList.add(b.marketId); breaksByAt.set(at, atList);
  }
  prevAt = -Infinity;
  for (const row of observations) {
    const availableMs = isoMs(row.availableAt, 'LEADLAG_POINT_CLOCK');
    if (availableMs < prevAt) throw Error('LEADLAG_POINT_ORDER');
    prevAt = availableMs;
    const meta = graph.exchanges.get(String(row.exchangeId));
    if (!meta || meta.marketId !== String(row.marketId)) { diagnostics.missingMetadata++; continue; }
    let sourceMs, expiryMs;
    try { sourceMs = Number(instantNs(row.sourceAt)) / 1e6; expiryMs = row.nextExpiryAt == null ? null : Number(instantNs(row.nextExpiryAt)) / 1e6; }
    catch { diagnostics.invalidPoints++; continue; }
    const bid = Number(row.bid), ask = Number(row.ask), bidSize = Number(row.bidSize), askSize = Number(row.askSize);
    if (!Number.isFinite(sourceMs) || sourceMs > availableMs || expiryMs !== null && (!Number.isFinite(expiryMs) || expiryMs <= availableMs)
      || !Number.isFinite(bid) || !Number.isFinite(ask) || bid <= 0 || ask >= 1 || ask <= bid
      || !Number.isSafeInteger(bidSize) || bidSize < 1 || !Number.isSafeInteger(askSize) || askSize < 1) {
      diagnostics.invalidPoints++; continue;
    }
    let sequence;
    try { sequence = BigInt(row.sequence); } catch { diagnostics.invalidPoints++; continue; }
    const point = {...row, meta, availableMs, sourceMs, expiryMs, bid, ask, bidSize, askSize, mid: (bid + ask) / 2, spread: ask - bid};
    const list = sourceSeries.get(meta.exchangeId) ?? [];
    const last = list.at(-1);
    if (last) {
      const previousSequence = BigInt(last.sequence);
      if (availableMs < last.availableMs || sequence < previousSequence || sequence === previousSequence && sourceMs <= last.sourceMs) throw Error('LEADLAG_VERSION_ORDER');
    }
    list.push(point); sourceSeries.set(meta.exchangeId, list); diagnostics.acceptedPoints++;
  }

  const maxLatency = cfg.latenciesMs.at(-1), maxHorizon = cfg.horizonsMs.at(-1);
  const splitPurgeMs = maxLatency + cfg.maxEntryDelayMs + maxHorizon + cfg.maxExitDelayMs;
  const histories = new Map(), lastDecision = new Map(), decisions = [];
  const byRace = new Map(), byStateParty = new Map(), byTypeParty = new Map();
  const add = (map, key, value) => { const xs = map.get(key) ?? []; xs.push(value); map.set(key, xs); };
  for (const meta of graph.exchanges.values()) {
    add(byRace, meta.raceId, meta);
    add(byStateParty, `${meta.stateGroup}|${meta.party}`, meta);
    add(byTypeParty, `${meta.type}|${meta.party}`, meta);
  }
  const maxLookback = cfg.lookbacksMs.at(-1);
  const allRows = [...sourceSeries.values()].flat().sort((a, b) => a.availableMs - b.availableMs || a.meta.exchangeId.localeCompare(b.meta.exchangeId));
  let cursor = 0;
  while (cursor < allRows.length) {
    const at = allRows[cursor].availableMs;
    let end = cursor + 1; while (end < allRows.length && allRows[end].availableMs === at) end++;
    // Invalidation at a timestamp applies before any same-time feature reads.
    const affected = breaksByAt.get(at) ?? new Set();
    if (affected.has(null)) histories.clear();
    else if (affected.size) for (const [exchangeId, list] of histories) if (affected.has(list.at(-1)?.meta.marketId)) histories.delete(exchangeId);

    for (let i = cursor; i < end; i++) {
      const p = allRows[i], meta = p.meta;
      if (stale(p, at, cfg.maxDataAgeMs)) { diagnostics.censoredStale++; continue; }
      if (affected.has(null) || affected.has(meta.marketId)) continue;
      const last = lastDecision.get(meta.exchangeId) ?? -Infinity;
      if (at - last < cfg.minDecisionSpacingMs) continue;
      lastDecision.set(meta.exchangeId, at);
      const isHeldout = heldOut(meta.stateGroup, cfg);
      const period = isHeldout ? (at >= cfg.splitAtMs ? 'heldout' : 'excluded_group')
        : (at + splitPurgeMs <= cfg.splitAtMs ? 'development' : 'excluded_time');
      if (period.startsWith('excluded')) continue;
      const features = {};
      const opponentIds = (byRace.get(meta.raceId) ?? []).filter(x => x.exchangeId !== meta.exchangeId && x.party !== meta.party).map(x => x.exchangeId);
      const stateIds = (byStateParty.get(`${meta.stateGroup}|${meta.party}`) ?? []).filter(x => x.raceId !== meta.raceId).map(x => x.exchangeId);
      const typeIds = (byTypeParty.get(`${meta.type}|${meta.party}`) ?? []).filter(x => x.stateGroup !== meta.stateGroup).map(x => x.exchangeId);
      for (const lookbackMs of cfg.lookbacksMs) {
        const own = peerMove(histories.get(meta.exchangeId), at, lookbackMs, cfg);
        features[`ownMomentum:${lookbackMs}`] = own;
        features[`raceOpponent:${lookbackMs}`] = aggregatePeerMove(opponentIds, histories, graph.exchanges, at, lookbackMs, cfg, -1);
        features[`stateSameParty:${lookbackMs}`] = aggregatePeerMove(stateIds, histories, graph.exchanges, at, lookbackMs, cfg, 1);
        features[`crossStateSameParty:${lookbackMs}`] = aggregatePeerMove(typeIds, histories, graph.exchanges, at, lookbackMs, cfg, 1, true);
      }
      if (Object.values(features).every(x => x === null)) { diagnostics.missingPeerFeature++; continue; }
      decisions.push({point:p, meta, at, period, features}); diagnostics.sampledDecisions++;
    }
    // Only after all decisions at this timestamp can its books become features.
    for (let i = cursor; i < end; i++) {
      const p = allRows[i];
      if (stale(p, at, cfg.maxDataAgeMs) || affected.has(null) || affected.has(p.meta.marketId)) continue;
      const xs = histories.get(p.meta.exchangeId) ?? []; xs.push(p);
      while (xs.length > 2 && at - xs[0].availableMs > maxLookback + cfg.maxPeerAgeMs + 1000) xs.shift();
      histories.set(p.meta.exchangeId, xs);
    }
    cursor = end;
  }

  const decisionRows = [];
  for (const d of decisions) {
    const series = sourceSeries.get(d.meta.exchangeId);
    const configs = [];
    for (const latencyMs of cfg.latenciesMs) for (const horizonMs of cfg.horizonsMs) {
      const entryAt = d.at + latencyMs;
      let ei = upperBound(series, entryAt, true);
      while (ei < series.length && stale(series[ei], series[ei].availableMs, cfg.maxDataAgeMs)) ei++;
      const entry = series[ei];
      if (!entry || entry.availableMs - entryAt > cfg.maxEntryDelayMs) { diagnostics.censoredEntry++; configs.push({latencyMs,horizonMs,outcome:null}); continue; }
      if (entry.askSize < cfg.minDepth) { diagnostics.censoredDepth++; configs.push({latencyMs,horizonMs,outcome:null}); continue; }
      const targetExitAt = entry.availableMs + horizonMs;
      let xi = upperBound(series, targetExitAt, true);
      while (xi < series.length && stale(series[xi], series[xi].availableMs, cfg.maxDataAgeMs)) xi++;
      const exit = series[xi];
      if (!exit || exit.availableMs - targetExitAt > cfg.maxExitDelayMs) { diagnostics.censoredExit++; configs.push({latencyMs,horizonMs,outcome:null}); continue; }
      if (exit.bidSize < cfg.minDepth) { diagnostics.censoredDepth++; configs.push({latencyMs,horizonMs,outcome:null}); continue; }
      const breaksForMarket = breaksByMarket.get(d.meta.marketId) ?? [], global = breaksByMarket.get(null) ?? [];
      if (hasBreakBetween(breaksForMarket,d.at,exit.availableMs) || hasBreakBetween(global,d.at,exit.availableMs)) { diagnostics.censoredBreak++; configs.push({latencyMs,horizonMs,outcome:null}); continue; }
      const grossMidMove = exit.mid - entry.mid, spreadCost = entry.spread / 2 + exit.spread / 2, net = exit.bid - entry.ask;
      if (Math.abs((grossMidMove - spreadCost) - net) > 1e-9) throw Error('LEADLAG_SPREAD_ACCOUNTING');
      const outcome = {grossMidMove, spreadCost, net, entrySpread:entry.spread, entryAt:entry.availableAt, exitAt:exit.availableAt, exitIndex:xi};
      configs.push({latencyMs,horizonMs,outcome});
    }
    decisionRows.push({...d, configs});
  }

  const featureNames = cfg.lookbacksMs.flatMap(l => ['raceOpponent','stateSameParty','crossStateSameParty','ownMomentum'].map(n => `${n}:${l}`));
  const rows = [];
  const developmentDecisionSamples=decisionRows.filter(x=>x.period==='development').length,heldoutDecisionSamples=decisionRows.filter(x=>x.period==='heldout').length;
  for (const latencyMs of cfg.latenciesMs) for (const horizonMs of cfg.horizonsMs) for (const feature of featureNames) {
    const dev = [];
    for (const d of decisionRows) {
      if (d.period !== 'development') continue;
      const config = d.configs.find(x => x.latencyMs === latencyMs && x.horizonMs === horizonMs), x = d.features[feature];
      if (config?.outcome && x !== null && Number.isFinite(x)) dev.push({x,y:config.outcome.grossMidMove,marketId:d.meta.marketId,stateGroup:d.meta.stateGroup});
    }
    const denom = dev.reduce((s,r)=>s+r.x*r.x,0), beta = dev.length>=cfg.minTrainingLabels&&denom>1e-12?dev.reduce((s,r)=>s+r.x*r.y,0)/denom:null;
    if (beta!==null&&(!Number.isFinite(beta)||Math.abs(beta)>100)) throw Error('LEADLAG_FIT_RANGE');
    let testLabels=0,selected=0,sumNet=0,wins=0,baseNet=0,baseCount=0,positiveGroups=0;
    const byGroup=new Map(),labelGroups=new Set();
    for (const d of decisionRows) {
      if (d.period!=='heldout') continue;
      const config=d.configs.find(x=>x.latencyMs===latencyMs&&x.horizonMs===horizonMs),x=d.features[feature],o=config?.outcome;
      if (!o || x===null||!Number.isFinite(x)) continue;
      testLabels++;baseNet+=o.net;baseCount++;labelGroups.add(d.meta.stateGroup);
      if (beta===null) continue;
      const predicted=beta*x, entry=d.point;
      if(predicted>entry.spread){selected++;sumNet+=o.net;if(o.net>0)wins++;const g=byGroup.get(d.meta.stateGroup)??{n:0,net:0};g.n++;g.net+=o.net;byGroup.set(d.meta.stateGroup,g);}
    }
    if(byGroup.size)positiveGroups=[...byGroup.values()].filter(x=>x.net/x.n>0).length;
    rows.push({latencyMs,horizonMs,feature,developmentDecisionSamples,heldoutDecisionSamples,developmentFeatureSamples:decisionRows.filter(d=>d.period==='development'&&d.features[feature]!==null).length,heldoutFeatureSamples:decisionRows.filter(d=>d.period==='heldout'&&d.features[feature]!==null).length,developmentLabels:dev.length,developmentSlope:beta===null?null:beta,heldoutLabels:testLabels,heldoutLabelCoverage:testLabels/(decisionRows.filter(d=>d.period==='heldout'&&d.features[feature]!==null).length||1),heldoutLabelGroups:labelGroups.size,heldoutRawNetMean:baseCount?baseNet/baseCount:null,selectedTrades:selected,selectedMeanNet:selected?sumNet/selected:null,selectedWinRate:selected?wins/selected:null,selectedGroups:byGroup.size,positiveSelectedGroups:positiveGroups,selectedNetOneShareSum:selected?sumNet:null,modelQualified:dev.length>=cfg.minTrainingLabels,positiveDescriptiveScreen:dev.length>=cfg.minTrainingLabels&&selected>=100&&byGroup.size>=5&&sumNet>0});
  }
  rows.sort((a,b)=>a.latencyMs-b.latencyMs||a.horizonMs-b.horizonMs||a.feature.localeCompare(b.feature));
  const holdoutGroups=cfg.heldoutStateGroups?[...cfg.heldoutStateGroups].sort():graph.stateGroups.filter(s=>heldOut(s,cfg));
  return {mode:'causal_cross_market_leadlag_quote_replay_v1',cutoff:new Date(cfg.splitAtMs).toISOString(),holdout:{unit:'stateGroup; all candidate races/contracts for a state kept together',groups:holdoutGroups,developmentStateCount:graph.stateGroups.length-holdoutGroups.length,heldoutStateCount:holdoutGroups.length},assumptions:{fees:'0.000000',entry:'first fresh next target book strictly after decision plus configured latency; buy one share at displayed ask',exit:'first fresh target book strictly after entry plus horizon; sell one share at displayed bid',minimumTopDepthShares:cfg.minDepth,staleBookMaxAgeMs:cfg.maxDataAgeMs,maxPeerReceiptAgeMs:cfg.maxPeerAgeMs,maxEntryDelayMs:cfg.maxEntryDelayMs,maxExitDelayMs:cfg.maxExitDelayMs,decisionSpacingMs:cfg.minDecisionSpacingMs,featureReadsStrictlyBeforeTimestampGroup:true,fullSpreadChargedByAskToFutureBid:true,trainingPurgeMs:splitPurgeMs},diagnostics,results:rows,limitations:['Hypothetical one-share quote replay, not observed fills or contest P&L.','Only source and receipt clocks represented in the capture are used; no latency extrapolation beyond the named scenarios.','Decision samples are spaced to limit within-market label overlap; markets in a state remain correlated. Group averages are descriptive, not independent significance tests.','Price co-movement and a positive simulated quote return do not establish that a live order could have filled at the displayed depth.','No fees are charged under the current challenge assumption; this does not verify future fee schedules.'],networkRequests:0,contestOrders:0,profitabilityEstablished:false};
}

function peerMove(rows, at, lookbackMs, cfg) {
  if (!rows?.length) return null;
  const endIndex = upperBound(rows, at, true) - 1;
  if (endIndex < 0) return null;
  const end = rows[endIndex];
  if (at - end.availableMs > cfg.maxPeerAgeMs || stale(end, end.availableMs, cfg.maxDataAgeMs)) return null;
  const targetStart = at - lookbackMs, startIndex = upperBound(rows, targetStart, true) - 1;
  if (startIndex < 0) return null;
  const start = rows[startIndex];
  if (targetStart - start.availableMs > cfg.maxPeerAgeMs || stale(start, start.availableMs, cfg.maxDataAgeMs)) return null;
  return end.mid - start.mid;
}

function aggregatePeerMove(exchangeIds, histories, metadata, at, lookbackMs, cfg, sign, byState = false) {
  const values = [], states = new Map();
  for (const exchangeId of exchangeIds) {
    const delta = peerMove(histories.get(exchangeId), at, lookbackMs, cfg);
    if (delta === null) continue;
    const value = sign * delta;
    if (!byState) values.push(value);
    else { const stateGroup = metadata.get(exchangeId)?.stateGroup; const xs = states.get(stateGroup) ?? []; xs.push(value); states.set(stateGroup, xs); }
  }
  if (byState) for (const xs of states.values()) values.push(median(xs));
  return values.length ? median(values) : null;
}
