"""Add verified original timing fields without changing sealed market values.

Reuse the sealed external observations and preserve a parent-manifest link;
reverify the original native byte chain. No network or account operations.
"""
import argparse,gzip,hashlib,json
from pathlib import Path
from moloch.learning_v2.compile_forward_development import native_rows,pin_prefix
from moloch.learning_v2.reference_adapters import millis

FIELDS=('native_event_received_at_ms','native_processed_at_ms')


def sha(raw):return hashlib.sha256(raw).hexdigest()


def enrich(row,verified):
    for key,value in row.items():
        if key not in verified or verified[key]!=value:raise ValueError('NATIVE_METADATA_EXTENSION_SOURCE_VALUE_CHANGED')
    if set(verified)-set(row)-set(FIELDS):raise ValueError('UNDECLARED_NATIVE_METADATA_EXTENSION')
    return {**row,**{key:verified[key]for key in FIELDS}}


def compile_metadata(base,output):
    base=Path(base).resolve();output=Path(output);raw=(base/'dataset-manifest.json').read_bytes();manifest=json.loads(raw)
    if json.loads((base/'COMPLETE.json').read_bytes())['manifestSha256']!=sha(raw):raise ValueError('ORIGINAL_BASE_MANIFEST_CHANGED')
    source=base/'observations.jsonl.gz'
    if sha(source.read_bytes())!=manifest['observationsSha256']:raise ValueError('ORIGINAL_BASE_OBSERVATIONS_CHANGED')
    protocol_raw=(base/'originals/protocol.original.json').read_bytes();protocol=json.loads(protocol_raw)
    if sha(protocol_raw)!=manifest['protocolSha256']:raise ValueError('ORIGINAL_CLOCK_PROTOCOL_CHANGED')
    output.mkdir(parents=True,exist_ok=False);originals=output/'originals';originals.mkdir()
    try:
        native=originals/'native.original.jsonl';pin=pin_prefix(base/'originals/native.original.jsonl',native)
        if pin['sha256']!=manifest['originalPrefixes'][0]['sha256']:raise ValueError('ORIGINAL_NATIVE_PREFIX_CHANGED')
        (originals/'protocol.original.json').write_bytes(protocol_raw)
        for name in ('reference-source-pin.original.json','kalshi-universe.original.json','polymarket-universe.original.json'):
            path=base/'originals'/name
            if path.exists():(originals/name).write_bytes(path.read_bytes())
        roles={k:tuple(map(millis,v))for k,v in protocol['calendarRolesUtc'].items()};verified={}
        def collect(row):
            identity=row['original_sha256']
            if identity in verified:raise ValueError('DUPLICATE_ORIGINAL_NATIVE_ROW')
            verified[identity]=row
        report=native_rows(native,roles,collect,protocol.get('nativeAvailabilityClock','receiver'))
        if report!=manifest['nativeTransportVerification']:raise ValueError('ORIGINAL_NATIVE_TRANSPORT_PROOF_CHANGED')
        path=output/'observations.jsonl.gz';changed=0;count=0
        with gzip.open(source,'rt',encoding='utf-8')as src,path.open('xb')as dest,gzip.GzipFile(fileobj=dest,mode='wb',mtime=0,compresslevel=1)as out:
            for line in src:
                row=json.loads(line);count+=1
                if row['venue']=='SIG'and row['kind']in('book','gap'):
                    original=verified.pop(row['original_sha256']);row=enrich(row,original);changed+=1
                out.write((json.dumps(row,separators=(',',':'),allow_nan=False)+'\n').encode())
        if verified:raise ValueError('ORIGINAL_NATIVE_ROWS_MISSING_FROM_SEALED_BASE')
        if sha(source.read_bytes())!=manifest['observationsSha256']or(base/'dataset-manifest.json').read_bytes()!=raw:
            raise ValueError('ORIGINAL_BASE_CHANGED_DURING_METADATA_EXTENSION')
        result={**manifest,'classification':'verified_snapshot_timing_metadata_extension_not_calibrated_or_test',
            'observationsSha256':sha(path.read_bytes()),'sourceBaseDataset':str(base),
            'sourceBaseManifestSha256':sha(raw),'sourceExternalOriginalsDirectory':str(base/'originals'),
            'metadataFieldsAdded':list(FIELDS),'nativeRowsMetadataVerified':changed,'totalRows':count,
            'marketValuesAndInformationClocksUnchanged':True,'physicalExchangeClockParityEstablished':False,
            'accountRequests':0,'productionAuthority':False}
        body=(json.dumps(result,indent=2)+'\n').encode();(output/'dataset-manifest.json').write_bytes(body)
        (output/'COMPLETE.json').write_text(json.dumps({'manifestSha256':sha(body)})+'\n')
        return result
    except BaseException:
        (output/'FAILED').write_text('Metadata extension incomplete; preserve diagnostics and originals.\n');raise


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--base-dataset',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    r=compile_metadata(a.base_dataset,a.output)
    print(json.dumps({k:r[k]for k in ('classification','sourceBaseManifestSha256','nativeRowsMetadataVerified','marketValuesAndInformationClocksUnchanged','accountRequests')}))
