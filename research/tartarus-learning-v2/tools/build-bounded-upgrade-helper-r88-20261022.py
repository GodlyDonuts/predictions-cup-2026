"""Build a move-only upgrade proposer with the SAME qualified engine identity.

Copies the checked native snapshot/chain/journal validation verbatim. Only the
final JSON container construction changes: move the retained entries instead of
serde-converting (cloning) their entire multi-gigabyte decoded representation.
No production bank is opened by this build or its finite parity fixtures.
"""
from pathlib import Path
import hashlib
import json
import os
import subprocess

ROOT = Path('/tmp/moloch-money-criterion-r88-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    Path('/proc/self/oom_score_adj').write_text('1000')
    os.nice(10)
    q = json.loads((ROOT / 'qualified.public.json').read_bytes())
    native = ROOT / 'libmoloch_replay_release.rlib'
    assert sha(native) == q['nativeLibrarySha256']
    session = ROOT / 'source/moloch/native/src/session.rs'
    text = session.read_text()
    functions = text[text.index('fn hash(v:'):text.index('fn options()')]
    chain = text[text.index('struct UpgradeChain {'):text.index('impl UpgradeChain {')]
    validation = text[text.index('    fn from_document('):text.index('    fn check_prefix(')]
    proposal = text[text.index('pub fn propose_live_engine_upgrade('):text.index('\npub struct Session {')]
    old = 'Ok(sealed(json!({"schemaVersion":1,"kind":"moloch_live_engine_upgrade_chain_v1","manifestHash":manifest["hash"],"upgrades":entries})))'
    new = '''let mut out=serde_json::Map::new();
    out.insert("schemaVersion".into(),json!(1));
    out.insert("kind".into(),json!("moloch_live_engine_upgrade_chain_v1"));
    out.insert("manifestHash".into(),manifest["hash"].clone());
    out.insert("upgrades".into(),Value::Array(entries));
    Ok(sealed(Value::Object(out)))'''
    assert proposal.count(old) == 1
    proposal = proposal.replace(old, new)
    original_main = (ROOT / 'propose-upgrade.rs').read_text()
    main_source = original_main[original_main.index('fn main()'):]
    main_source = main_source.replace('fn main()->Result<(),', 'fn main()->std::result::Result<(),')
    main_source = main_source.replace('moloch_replay::session::propose_live_engine_upgrade(', 'propose_live_engine_upgrade(')
    source = ROOT / 'propose-upgrade-bounded.rs'
    source.write_text('''use std::{collections::BTreeMap,env,fs::{self,File,OpenOptions},io::{BufRead,BufReader,Write},path::Path};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use moloch_replay::{journal::Journal,session::{ExclusiveLock,engine_identity}};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error>>;
''' + functions + chain + 'impl UpgradeChain {\n' + validation + '\n}\n' + proposal + main_source)
    args = json.loads((ROOT / 'native-release-command.json').read_bytes())
    dependencies = []
    for i, value in enumerate(args[:-1]):
        if value == '-L' or value == '--extern' and args[i + 1].split('=')[0] in ('serde_json', 'sha2', 'chrono'):
            dependencies.extend([value, args[i + 1]])
    binary = ROOT / 'propose-upgrade-bounded'
    subprocess.run([RUST, '--edition=2024', '-C', 'opt-level=3', '-C', 'lto=thin',
        '-C', 'codegen-units=1', '-C', 'embed-bitcode=yes', str(source),
        '--extern', 'moloch_replay=' + str(native), *dependencies, '-o', str(binary)], check=True)
    identity = subprocess.check_output([str(binary), 'identity'], text=True).strip()
    assert identity == subprocess.check_output([str(ROOT / 'propose-upgrade'), 'identity'], text=True).strip()
    # Small immutable byte/seal parity fixture. This is not a contest account.
    fixture = ROOT / 'bounded-proposer-fixture'
    fixture.mkdir(mode=0o700, exist_ok=False)
    canonical = lambda v: json.dumps(v, sort_keys=True, separators=(',', ':')).encode()
    seal = lambda v: dict(v, hash=hashlib.sha256(canonical(v)).hexdigest())
    save = lambda name, value: (fixture / name).write_bytes(canonical(value) + b'\n')
    manifest = seal(dict(mode='rust_live_session', engineIdentity='a' * 64))
    head = seal(dict(manifestHash=manifest['hash'], sequence=0, previous=''))
    report = seal(dict(manifestHash=manifest['hash'], sequence=0, previous='', report={}))
    entry = seal(dict(schemaVersion=1, manifestHash=manifest['hash'], fromEngineIdentity='a' * 64,
        toEngineIdentity='b' * 64, previousUpgradeHash='', oldHead=head, oldReportEnvelope=report,
        journalPrefix=dict(bytes=0, sha256=hashlib.sha256(b'').hexdigest()),
        approval=dict(role='human_authorized_execution_owner', previousProcessStopped=True,
                      oldBinarySha256='c' * 64, approvedAt='2026-10-22T07:00:00Z')))
    document = seal(dict(schemaVersion=1, kind='moloch_live_engine_upgrade_chain_v1',
                         manifestHash=manifest['hash'], upgrades=[entry]))
    for name, value in [('manifest.json', manifest), ('head.json', head), ('report.json', report),
                        ('engine-upgrade-v1.json', document)]:
        save(name, value)
    (fixture / 'events.jsonl').write_bytes(b'')
    arguments = [str(fixture), 'd' * 64, '2026-10-22T07:01:00Z']
    for program, output in [('propose-upgrade', 'original.json'), ('propose-upgrade-bounded', 'bounded.json')]:
        subprocess.run([str(ROOT / program), *arguments, str(fixture / output)], check=True)
    assert (fixture / 'original.json').read_bytes() == (fixture / 'bounded.json').read_bytes()
    original_pins = {n: sha(fixture / n) for n in ('manifest.json', 'head.json', 'report.json', 'events.jsonl')}
    document['upgrades'][0]['oldReportEnvelope']['hash'] = '0' * 64
    document.pop('hash')
    save('engine-upgrade-v1.json', seal(document))
    for program in ('propose-upgrade', 'propose-upgrade-bounded'):
        output = fixture / (program + '-invalid.json')
        result = subprocess.run([str(ROOT / program), *arguments, str(output)], capture_output=True, text=True)
        assert result.returncode != 0 and 'UPGRADE_HASH_MISMATCH' in result.stderr and not output.exists()
    assert all(sha(fixture / name) == value for name, value in original_pins.items())
    proof = dict(engineIdentity=identity, helperSha256=sha(binary), helperSourceSha256=sha(source),
        nativeLibrarySha256=sha(native), checkedNativeSessionSourceSha256=sha(session),
        proposalConstructionChange='move original entries into final Value::Array; no serde clone',
        smallOriginalProposerByteParity=True, malformedPriorReportRejectedByBoth=True,
        fixtureFinancialFilesUnchanged=True, productionBankOpened=False, accountRequests=0)
    (ROOT / 'bounded-helper.public.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    main()
