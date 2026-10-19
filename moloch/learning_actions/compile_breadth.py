"""Seal original raw SIG snapshots beside unchanged recorded external evidence.

Retrospective development only. No account access, trading, test qualification,
invented historical laptop receipts or assumption of contract equivalence.
"""
import argparse,gzip,hashlib,json,shutil
from collections import Counter
from pathlib import Path
from moloch.learning_flow.episodes import FlowTape,FEATURE_SCHEMA
from moloch.learning_flow.features import Flow,FEATURE_NAMES
from .breadth_source import frames,Decoder


def sha(raw):return hashlib.sha256(raw).hexdigest()


def write_rows(path,rows):
    with path.open('xb')as f,gzip.GzipFile(fileobj=f,mode='wb',mtime=0,compresslevel=1)as z:
        for row in rows:z.write((json.dumps(row,separators=(',',':'),allow_nan=False)+'\n').encode())


def seal(directory,manifest):
    raw=(json.dumps(manifest,indent=2)+'\n').encode();(directory/'dataset-manifest.json').write_bytes(raw)
    (directory/'COMPLETE.json').write_text(json.dumps({'manifestSha256':sha(raw)})+'\n')
    return sha(raw)


def compile_breadth(parent,source,output,archive_sha256,source_manifest_sha256,tournament):
    parent=Path(parent).resolve();source=Path(source).resolve();output=Path(output).resolve()
    original=FlowTape(parent);parent_raw=(parent/'dataset-manifest.json').read_bytes()
    original_base=Path(original.manifest['baseDataset']);base_raw=(original_base/'dataset-manifest.json').read_bytes()
    base_manifest=json.loads(base_raw)
    if sha(base_raw)!=original.manifest['baseManifestSha256']:raise ValueError('BREADTH_EXTERNAL_BASE_CHANGED')
    output.mkdir(parents=True,exist_ok=False)
    try:
        decoder=Decoder(tournament);rows=[];prints=[]
        for event,original_sha,at,role in frames(source,archive_sha256,source_manifest_sha256):
            books,trades=decoder.decode(event,original_sha,at,role);rows.extend(books);prints.extend(trades)
        # Keep exact sealed external observations; use raw frames for SIG state.
        references=[row for row in original.rows if row['venue']in('Kalshi','Polymarket')]
        rows.extend(references);rows.sort(key=lambda r:r['available_at_ms'])
        if not decoder.books:raise ValueError('BREADTH_NO_ORIGINAL_BOOKS')
        base=output/'base';base.mkdir();originals=base/'originals';originals.mkdir()
        for p in (original_base/'originals').iterdir():
            if p.is_file():shutil.copyfile(p,originals/p.name)
        write_rows(base/'observations.jsonl.gz',rows)
        metadata={**base_manifest,'classification':'raw_realtime_SIG_with_original_external_development_not_calibrated_or_test',
            'observationsSha256':sha((base/'observations.jsonl.gz').read_bytes()),'totalRows':len(rows),
            'observationCounts':dict(Counter('/'.join((r['calendar_role'],r['venue'],r['kind']))for r in rows)),
            'fitSigExchanges':len({r['exchange_id']for r in rows if r['kind']=='book'and r['calendar_role']=='fit'}),
            'marketValuesAndInformationClocksUnchanged':False,
            'metadataFieldsAdded':['snapshot_origin','original_realtime_received_at_ms'],
            'nativeRowsMetadataVerified':0,
            'legacyParentNativeRowsMetadataVerified':base_manifest.get('nativeRowsMetadataVerified',0),
            'nativeAvailabilityClock':'not_used_for_current_SIG_snapshots_original_realtime_receiver_clock',
            'sourceParentFlowManifestSha256':sha(parent_raw),'sourceExternalOriginalsDirectory':
                base_manifest.get('sourceExternalOriginalsDirectory',str(original_base/'originals')),
            'sourceOriginalBreadthSlicesDirectory':str(source),'originalBreadthArchiveSha256':archive_sha256,
            'originalBreadthSourceManifestSha256':source_manifest_sha256,
            'sigObservationSource':'original_official_sig_realtime_frame',
            'legacyNativeProofNotCurrentSIGObservationSource':True,
            'normalizationCounts':dict(decoder.counts),'originalSIGExchanges':len(decoder.books),
            'retrospectiveSourceReceiverClockNotHistoricalLaptopReceipt':True,
            'sourceProcessingLatencyUnknown':True,'externalObservationsUnchanged':True,
            'fullHistoryReady':False,'independentTestEligible':False,'productionAuthority':False,'accountRequests':0}
        base_sha=seal(base,metadata)
        combined=sorted([*prints,*original.flow_rows],key=lambda r:r['available_at_ms']);verifier=Flow();flow=[]
        for row in combined:
            if verifier.accept(row['available_at_ms'],row):flow.append(row)
        write_rows(output/'trade-flow.jsonl.gz',flow)
        result={**original.manifest,'classification':'raw_breadth_snapshot_and_engine_print_development_not_live_or_calibrated',
            'featureSchema':FEATURE_SCHEMA,'additionalFeatures':FEATURE_NAMES,'baseDataset':str(base),
            'baseManifestSha256':base_sha,'baseClassification':metadata['classification'],
            'sourceParentFlowManifestSha256':sha(parent_raw),'tradeRowsSha256':sha((output/'trade-flow.jsonl.gz').read_bytes()),
            'tradeRows':len(flow),'normalizationCounts':dict(decoder.counts),'originalSIGExchanges':len(decoder.books),
            'originalTradeNamespaces':{'sig_engine_match':len(flow)},
            'legacyNativeProofNotCurrentSIGObservationSource':True,
            'executableLiquidityOrFinancialStateChanged':True,'financialStateChanged':False,
            'executableLiquidityChangedRelativeToParent':True,
            'originalBreadthArchiveSha256':archive_sha256,'originalBreadthSourceManifestSha256':source_manifest_sha256,
            'compilerSourceSha256':sha(Path(__file__).read_bytes()),
            'decoderSourceSha256':sha(Path(__file__).with_name('breadth_source.py').read_bytes()),
            'allHistoricalOwnTradesExcluded':False,'heldoutFeaturesIncluded':False,
            'physicalExchangeClockParityEstablished':False,'productionAuthority':False,'accountRequests':0}
        if (parent/'dataset-manifest.json').read_bytes()!=parent_raw or(original_base/'dataset-manifest.json').read_bytes()!=base_raw:
            raise ValueError('BREADTH_PARENT_CHANGED_DURING_COMPILE')
        seal(output,result)
        return result
    except BaseException:
        (output/'FAILED').write_text('Raw breadth compilation incomplete; retain diagnostics.\n');raise


if __name__=='__main__':
    p=argparse.ArgumentParser()
    for name in('parent','source','output'):p.add_argument('--'+name,type=Path,required=True)
    for name in('archive-sha256','source-manifest-sha256','tournament'):p.add_argument('--'+name,required=True)
    a=p.parse_args();r=compile_breadth(a.parent,a.source,a.output,a.archive_sha256,a.source_manifest_sha256,a.tournament)
    print(json.dumps({k:r[k]for k in('classification','tradeRows','normalizationCounts','originalSIGExchanges','accountRequests')}))
