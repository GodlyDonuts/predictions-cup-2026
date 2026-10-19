"""Pair verified official prints with an immutable completed book/reference tape.

The existing byte/clock verifier runs with a private extractor binding. Its
module globals and the baseline compiler remain unchanged for parallel studies.
"""
import argparse,gzip,hashlib,inspect,json,types
from collections import Counter
from pathlib import Path
from moloch.learning_v2.episodes import Tape
from moloch.learning_v2.compile_forward_development import native_rows
from moloch.learning_v2.reference_adapters import millis
from .features import trade,Flow,FEATURE_NAMES
from .episodes import FEATURE_SCHEMA


def sha(raw):return hashlib.sha256(raw).hexdigest()


def trade_reader(extractor=trade):
    return types.FunctionType(native_rows.__code__,{**native_rows.__globals__,'extract':extractor},
                              'verified_trade_rows',native_rows.__defaults__)


def compile_flow(base,output):
    base=Path(base).resolve();output=Path(output);t=Tape(base)
    raw=(base/'originals/protocol.original.json').read_bytes();protocol=json.loads(raw)
    if sha(raw)!=t.manifest['protocolSha256']:raise ValueError('ORIGINAL_ROLE_MANIFEST_CHANGED')
    roles={k:tuple(map(millis,v))for k,v in protocol['calendarRolesUtc'].items()};native=base/'originals/native.original.jsonl'
    pin=t.manifest['originalPrefixes'][0]
    if sha(native.read_bytes())!=pin['sha256']:raise ValueError('ORIGINAL_NATIVE_PREFIX_CHANGED')
    rows=[];dedup=Flow();namespaces=Counter()
    def emit(row):
        if dedup.accept(row['available_at_ms'],row):rows.append(row)
    def extract(event,original,available):
        if event['type']=='trade':namespaces[str(event.get('idNamespace'))]+=1
        return trade(event,original,available)
    verified=trade_reader(extract)(native,roles,emit,protocol.get('nativeAvailabilityClock','receiver'))
    if verified!=t.manifest['nativeTransportVerification']:raise ValueError('ORIGINAL_TRANSPORT_VERIFICATION_CHANGED')
    output.mkdir(parents=True,exist_ok=False)
    p=output/'trade-flow.jsonl.gz'
    with p.open('xb')as f,gzip.GzipFile(fileobj=f,mode='wb',mtime=0,compresslevel=1)as z:
        for row in rows:z.write((json.dumps(row,separators=(',',':'))+'\n').encode())
    manifest={'classification':'trade_flow_extension_of_verified_base_not_live_or_calibrated',
        'featureSchema':FEATURE_SCHEMA,'additionalFeatures':FEATURE_NAMES,'baseDataset':str(base),
        'baseManifestSha256':t.manifest_sha256,'baseClassification':t.manifest['classification'],
        'tradeRowsSha256':sha(p.read_bytes()),'tradeRows':len(rows),'originalNativePrefixSha256':pin['sha256'],
        'originalTradeNamespaces':dict(namespaces),'reportingTapeNotAddedToEngineMatchVolume':True,
        'verifierFunctionSourceSha256':sha(inspect.getsource(native_rows).encode()),
        'flowCompilerSourceSha256':sha(Path(__file__).read_bytes()),
        'featureSourceSha256':sha(Path(__file__).with_name('features.py').read_bytes()),
        'heldoutFeaturesIncluded':False,'aggressorSideInferredOrInvented':False,
        'executableLiquidityOrFinancialStateChanged':False,'accountRequests':0,'productionAuthority':False}
    body=(json.dumps(manifest,indent=2)+'\n').encode();(output/'dataset-manifest.json').write_bytes(body)
    (output/'COMPLETE.json').write_text(json.dumps({'manifestSha256':sha(body)})+'\n',encoding='utf-8')
    return manifest


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--base-dataset',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    print(json.dumps(compile_flow(a.base_dataset,a.output)))
