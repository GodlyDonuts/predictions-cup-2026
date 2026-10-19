"""Original raw SIG frames and immutable external receipts, development only.

No requests, synthetic historical receipts, held-out rows or gap imputation.
Missing coverage fails the frozen cohort instead of moving its boundaries.
"""
import argparse,hashlib,json,stat
from collections import Counter,defaultdict
from pathlib import Path
from .breadth_source import frames,Decoder
from .compile_breadth import write_rows,seal
from moloch.learning_v2.compile_forward_development import gzip_rows
from moloch.learning_v2.reference_adapters import millis,kalshi_references,polymarket_references
from moloch.research.compile_native_learning_history import role_at
from moloch.learning_flow.episodes import FEATURE_SCHEMA
from moloch.learning_flow.features import FEATURE_NAMES,Flow

STRICT='retrospective_development_only_not_four_day_evolution_or_independent_test'
OUTAGE='retrospective_development_with_explicit_unknown_liquidity_not_four_day_qualification'


def digest(raw):return hashlib.sha256(raw).hexdigest()


def verify_snapshot(entry):
    p=Path(entry['path']);before=p.lstat()
    if not stat.S_ISREG(before.st_mode)or p.is_symlink():raise ValueError('REGULAR_IMMUTABLE_REFERENCE_SOURCE')
    if before.st_size!=entry['source_size']:raise ValueError('ARCHIVED_REFERENCE_SIZE_CHANGED')
    with p.open('rb')as f:actual=hashlib.file_digest(f,'sha256').hexdigest()
    if actual!=entry['sha256']:raise ValueError('ARCHIVED_REFERENCE_BYTES_CHANGED')
    return p,before


def reference_rows(index,kuniverse,puniverse,roles,pins):
    km={m['ticker']:m for m in kuniverse['markets']};analogs={m['ticker']for m in kuniverse.get('candidateAnalogMarkets',[])}
    pm={m['assetId']:m for m in puniverse['outcomes']};seen=set()
    for entry in index['files']:
        identity=(entry['source'],entry['filename'])
        if identity in seen:raise ValueError('DUPLICATE_ARCHIVED_SOURCE_FILENAME')
        seen.add(identity);venue=entry['venue']
        if venue not in('Kalshi','Polymarket'):raise ValueError('EXPLICIT_REFERENCE_VENUE')
        p,before=verify_snapshot(entry);total=0;selected=0
        # These are immutable completed archive members, never active prefixes.
        for raw in gzip_rows(p,False):
            total+=1;row=json.loads(raw);receipt=row.get('receipt',row)
            if not isinstance(receipt,dict)or'receivedAt'not in receipt or'bodyText'not in receipt:continue
            at=max(millis(receipt['receivedAt']),millis(row['firstAvailableAt'])if'firstAvailableAt'in row else 0)
            role=role_at(at,roles)
            if role not in('fit','search'):continue
            if venue=='Kalshi':
                mapping=km.get(row.get('ticker'))
                if mapping is None or not mapping.get('sigMappings')or row.get('ticker')in analogs:continue
                values=kalshi_references(raw,mapping['sigMappings'],at,mapping_kind='direct_party_reference')
            else:
                identity=row.get('identity')
                if not identity:continue
                if pm.get(identity.get('assetId'))!=identity:raise ValueError('ORIGINAL_POLYMARKET_MAPPING_IDENTITY_CHANGED')
                values=polymarket_references(raw,at,invalid_market_data='gap')
            for value in values:
                selected+=1;yield {**value,'calendar_role':role}
        after=p.lstat()
        if(before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns)!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns):
            raise ValueError('ARCHIVED_REFERENCE_CHANGED_DURING_COMPILE')
        pins.append({**entry,'originalRowsVerified':total,'selectedNormalizedRows':selected,'completeGzipVerified':True})


def coverage(rows,roles,maximum_gap,*,allow_recorded_sig_resets=False):
    if type(maximum_gap)is not int or maximum_gap<=0:raise ValueError('EXPLICIT_POSITIVE_COVERAGE_GAP')
    if type(allow_recorded_sig_resets)is not bool:raise ValueError('EXPLICIT_RECORDED_OUTAGE_PROFILE')
    times=defaultdict(list)
    resets=defaultdict(list)
    for row in rows:
        role=row['calendar_role'];lo,hi=roles[role];at=row['available_at_ms']
        if not lo<=at<hi:raise ValueError('ROW_OUTSIDE_FROZEN_DEVELOPMENT_ROLE')
        if row['kind']in('book','reference'):times[role,row['venue']].append(at)
        if row['venue']=='SIG'and row.get('gap_reason')=='original_stream_reset'and row.get('snapshot_origin')=='original_official_sig_realtime_frame':
            resets[role].append(at)
    result={}
    for role,(lo,hi)in roles.items():
        for venue in('SIG','Kalshi','Polymarket'):
            values=sorted(times[role,venue])
            if not values:raise ValueError('ARCHIVED_COHORT_NO_VALID_SOURCE:'+role+':'+venue)
            pairs=list(zip([lo]+values,values+[hi]))
            gap_start,gap_end=max(pairs,key=lambda ab:ab[1]-ab[0])
            gap=gap_end-gap_start
            unknown=[]
            for a,b in pairs:
                if b-a<=maximum_gap:continue
                witnesses=[at for at in resets[role]if a<at<=b]
                if not(allow_recorded_sig_resets and venue=='SIG'and witnesses):
                    exc=ValueError('ARCHIVED_COHORT_COVERAGE_GAP:'+role+':'+venue)
                    exc.coverage_gap={'role':role,'venue':venue,'start_ms':a,'end_ms':b,'duration_ms':b-a}
                    raise exc
                unknown.append({'start_ms':a,'end_ms':b,'duration_ms':b-a,'recorded_reset_at_ms':witnesses,
                                'continuous_hidden_liquidity_known':False,'observable_reset_backdated':False})
            result[role+'/'+venue]={'validRows':len(values),'maximumObservedGapMs':gap,'firstAtMs':values[0],
                'lastAtMs':values[-1],'completeObservedCoverage':not unknown,'recordedUnknownIntervals':unknown}
    return result


def compile_cohort(source,index_path,protocol_path,kuniverse_path,puniverse_path,output,tournament):
    protocol_raw=Path(protocol_path).read_bytes();protocol=json.loads(protocol_raw)
    if protocol['classification']not in(STRICT,OUTAGE):
        raise ValueError('EXPLICIT_RETROSPECTIVE_DEVELOPMENT_COHORT')
    outage=protocol['classification']==OUTAGE
    lifetime=protocol.get('worldBookLifetimeMs')
    if outage and(type(lifetime)is not int or not 0<lifetime<=protocol['maximumBoundaryOrObservationGapMs']):
        raise ValueError('OUTAGE_PROFILE_REQUIRES_CAUSAL_LIQUIDITY_EXPIRY')
    if not outage and lifetime is not None:raise ValueError('STRICT_COHORT_CANNOT_REINTERPRET_SOURCE_GAPS')
    if protocol['independentTestIncluded']is not False or protocol['productionAuthority']is not False:
        raise ValueError('NO_TEST_OR_PRODUCTION_AUTHORITY')
    if tournament!=protocol['tournamentId']:raise ValueError('FROZEN_TOURNAMENT_SCOPE')
    roles={k:tuple(map(millis,v))for k,v in protocol['calendarRolesUtc'].items()}
    if set(roles)!={'fit','search'}or not roles['fit'][0]<roles['fit'][1]<roles['search'][0]<roles['search'][1]:
        raise ValueError('FROZEN_CHRONOLOGICAL_DEVELOPMENT_ROLES')
    for interval in protocol['protectedCalendarsExcludedUtc']:
        lo,hi=map(millis,interval)
        if any(a<hi and lo<b for a,b in roles.values()):raise ValueError('PROTECTED_CALENDAR_OVERLAP')
    source=Path(source);raw_manifest=(source/'source-manifest.json').read_bytes();source_manifest=json.loads(raw_manifest)
    if {k:list(v)for k,v in roles.items()}!=source_manifest['roles_ms']:raise ValueError('SOURCE_PROTOCOL_CALENDAR_MISMATCH')
    index_raw=Path(index_path).read_bytes();index=json.loads(index_raw)
    ku=Path(kuniverse_path).read_bytes();pu=Path(puniverse_path).read_bytes()
    if digest(ku)!=protocol['kalshiUniverseSha256']or digest(pu)!=protocol['polymarketUniverseSha256']:
        raise ValueError('FROZEN_REFERENCE_UNIVERSES_CHANGED')
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    try:
        rows=[];trades=[];decoder=Decoder(tournament)
        for event,original,at,role in frames(source,source_manifest['archive_sha256'],digest(raw_manifest)):
            books,prints=decoder.decode(event,original,at,role);rows.extend(books);trades.extend(prints)
        pins=[];rows.extend(reference_rows(index,json.loads(ku),json.loads(pu),roles,pins))
        observed_coverage=coverage(rows,roles,protocol['maximumBoundaryOrObservationGapMs'],allow_recorded_sig_resets=outage)
        counts=dict(Counter('/'.join((r['calendar_role'],r['venue'],r['kind']))for r in rows))
        sig={role:len({r['exchange_id']for r in rows if r['calendar_role']==role and r['kind']=='book'})for role in roles}
        references={role:{venue:len({r['market_id']for r in rows if r['calendar_role']==role and r['venue']==venue and r['kind']=='reference'})for venue in('Kalshi','Polymarket')}for role in roles}
        if any(n<protocol['minimumOriginalSigExchanges']for n in sig.values())or any(n<protocol['minimumMappedReferenceMarketsPerExternalVenue']for v in references.values()for n in v.values()):
            raise ValueError('ARCHIVED_COHORT_UNIVERSE_COVERAGE')
        rows.sort(key=lambda r:r['available_at_ms']);trades.sort(key=lambda r:r['available_at_ms'])
        verifier=Flow();flow=[r for r in trades if verifier.accept(r['available_at_ms'],r)]
        base=output/'base';base.mkdir();originals=base/'originals';originals.mkdir()
        for name,raw in [('protocol.original.json',protocol_raw),('source-index.original.json',index_raw),('kalshi-universe.original.json',ku),('polymarket-universe.original.json',pu),('sig-source-manifest.original.json',raw_manifest)]:
            (originals/name).write_bytes(raw)
        write_rows(base/'observations.jsonl.gz',rows)
        common={'classification':'retrospective_original_three_venue_development_not_independent_test',
            'protocolSha256':digest(protocol_raw),'sourceIndexSha256':digest(index_raw),
            'kalshiUniverseSha256':digest(ku),'polymarketUniverseSha256':digest(pu),
            'calendarRolesMs':roles,'observationCounts':counts,'sourceCoverage':observed_coverage,
            'originalSigExchangesByRole':sig,'mappedReferenceMarketsByRole':references,
            'originalBreadthArchiveSha256':source_manifest['archive_sha256'],
            'originalBreadthSourceManifestSha256':digest(raw_manifest),'sourceOriginalBreadthSlicesDirectory':str(source.resolve()),
            'originalsStoredByReferenceNotDuplicated':True,'originalPrefixes':pins,
            'referenceContractEquivalenceVerified':False,'fullHistoryReady':False,'independentTestEligible':False,
            'sourceCoveragePolicy':'recorded_reset_unknown_liquidity_development_v1'if outage else'strict_continuous_observation_v1',
            'requiredWorldBookLifetimeMs':lifetime,
            'completeObservedSourceCoverage':all(v['completeObservedCoverage']for v in observed_coverage.values()),
            'heldoutFeaturesIncluded':False,'executionCalibrated':False,'accountRequests':0,'productionAuthority':False}
        base_sha=seal(base,{**common,'observationsSha256':digest((base/'observations.jsonl.gz').read_bytes()),'totalRows':len(rows)})
        write_rows(output/'trade-flow.jsonl.gz',flow)
        result={**common,'featureSchema':FEATURE_SCHEMA,'additionalFeatures':FEATURE_NAMES,'baseDataset':str(base.resolve()),
            'baseManifestSha256':base_sha,'tradeRowsSha256':digest((output/'trade-flow.jsonl.gz').read_bytes()),
            'tradeRows':len(flow),'originalSIGExchanges':len(decoder.books),'allHistoricalOwnTradesExcluded':False,
            'compilerSourceSha256':digest(Path(__file__).read_bytes()),'decoderSourceSha256':digest(Path(__file__).with_name('breadth_source.py').read_bytes())}
        if Path(protocol_path).read_bytes()!=protocol_raw or Path(index_path).read_bytes()!=index_raw or Path(kuniverse_path).read_bytes()!=ku or Path(puniverse_path).read_bytes()!=pu:
            raise ValueError('COHORT_PROTOCOL_OR_INDEX_CHANGED')
        seal(output,result);return result
    except BaseException as exc:
        (output/'FAILED').write_text('Incomplete frozen cohort; retain source evidence and diagnostics.\n')
        (output/'FAILED.json').write_text(json.dumps({'exception':type(exc).__name__,'reason':str(exc),
            'coverageGap':getattr(exc,'coverage_gap',None),
            'protocolSha256':digest(protocol_raw),'accountRequests':0,'productionAuthority':False})+'\n');raise


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for key in('source','index','protocol','kalshi-universe','polymarket-universe','output'):
        p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--tournament',required=True);a=p.parse_args()
    result=compile_cohort(a.source,a.index,a.protocol,a.kalshi_universe,a.polymarket_universe,a.output,a.tournament)
    print(json.dumps({k:result[k]for k in('tradeRows','originalSigExchangesByRole','mappedReferenceMarketsByRole','sourceCoverage',
                                       'completeObservedSourceCoverage','requiredWorldBookLifetimeMs','accountRequests')}),flush=True)
