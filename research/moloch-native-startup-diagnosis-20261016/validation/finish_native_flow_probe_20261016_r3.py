"""Complete only the failed offline probe link. No banks or production actions."""
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

HELPER = Path('/tmp/build_native_flow_candidate_20261016_r2.py')
HELPER_SHA = 'd2cbd3fbb7feabb120ad1d00ae32b22237fc1f4b4f372188c52c7d21c0bb70da'
ROOT = Path('/tmp/moloch-native-flow-build-b2202660-20261016-r3')
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()

def main():
    assert sys.platform.startswith('linux') and len(sys.argv) == 2 and sha(__file__) == sys.argv[1]
    assert sha(HELPER) == HELPER_SHA
    spec = importlib.util.spec_from_file_location('reviewed_r2', HELPER)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    assert not os.environ.get('RUSTFLAGS') and not os.environ.get('CARGO_ENCODED_RUSTFLAGS')
    ROOT.mkdir(mode=0o700)
    evidence = {'offlineOnly': True, 'productionAuthorization': False,
                'bankReplayRun': False, 'bridgeProposed': False,
                'startedAt': dt.datetime.now(dt.timezone.utc).isoformat(),
                'linkRepair': 'Add actual serde artifact parent as a dependency search directory; preserve R2 failed link.',
                'checks': {}, 'artifacts': {}}
    for name in ('compiler', 'native-library', 'native-release', 'baseline-probe-build', 'baseline-identity', 'candidate-probe-build'):
        receipt = json.loads((m.ROOT / (name + '-receipt.json')).read_bytes())
        assert m.pinned(m.ROOT / (name + '.log')) == receipt['log']
        assert receipt['exitCode'] == (1 if name == 'candidate-probe-build' else 0)
        evidence['checks']['original-r2-' + name] = receipt
    assert 'test result: ok. 77 passed; 0 failed; 1 ignored;' in (m.ROOT / 'native-library.log').read_text()
    assert (m.ROOT / 'native-library.log').read_text().count('test engine::activity_window_tests::') == 5
    m.pinned(m.BASE / 'source-manifest.json', m.MANIFEST_SHA)
    m.pinned(m.BASE / 'release.log', m.RELEASE_SHA)
    m.pinned(m.BASE / 'validation.json', m.VALIDATION_SHA)
    manifest = json.loads((m.BASE / 'source-manifest.json').read_bytes())
    for name, digest in manifest['sourceHashes'].items():
        m.pinned(m.BASE / name, digest)
    old_native, new_native = m.BASE / 'moloch/native', m.ROOT / 'moloch/native'
    old_id, old_sources = m.identity(old_native)
    new_id, new_sources = m.identity(new_native)
    assert old_id == m.OLD and new_id != old_id
    assert [n for n in m.INPUTS if old_sources[n] != new_sources[n]] == ['src/engine.rs']
    import zipfile
    m.pinned(m.ARCHIVE, m.ARCHIVE_SHA)
    with zipfile.ZipFile(m.ARCHIVE) as z:
        assert z.read('engine.rs') == (new_native / 'src/engine.rs').read_bytes()
        assert z.read('offline_probe.rs') == (m.ROOT / 'offline_probe.rs').read_bytes()
    old_rlib, old_art = m.artifacts(m.BASE / 'release.log', 'moloch_replay', old_native / 'Cargo.toml')
    old_json, old_json_art = m.artifacts(m.BASE / 'release.log', 'serde_json')
    new_rlib, new_art = m.artifacts(m.ROOT / 'native-release.log', 'moloch_replay', new_native / 'Cargo.toml')
    new_json, new_json_art = m.artifacts(m.ROOT / 'native-release.log', 'serde_json')
    assert new_art['profile'] == old_art['profile']
    assert new_json_art['features'] == old_json_art['features']
    assert sorted(new_json_art['features']) == ['arbitrary_precision', 'default', 'std']
    compiler = Path.home() / '.cargo/bin/rustc'
    def run(name, args, seconds=1800):
        start = time.monotonic()
        with (ROOT / (name + '.log')).open('xb') as log:
            p = subprocess.Popen(['nice', '-n', '19', *map(str, args)], stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = p.wait(timeout=seconds)
            except subprocess.TimeoutExpired:
                os.killpg(p.pid, signal.SIGTERM)
                try: p.wait(timeout=5)
                except subprocess.TimeoutExpired: os.killpg(p.pid, signal.SIGKILL); p.wait()
                code = 'timeout'
        row = {'exitCode': code, 'seconds': time.monotonic() - start, 'log': m.pinned(ROOT / (name + '.log'))}
        evidence['checks'][name] = row
        (ROOT / (name + '-receipt.json')).write_text(json.dumps(row, indent=2) + '\n')
        assert code == 0, name
        return ROOT / (name + '.log')
    candidate_probe = ROOT / 'candidate-probe'
    run('candidate-probe-build', [compiler, '--edition=2024', '-C', 'opt-level=3', '-C', 'lto=thin', '-C', 'codegen-units=1',
        m.ROOT / 'offline_probe.rs', '-L', 'dependency=' + str(new_rlib.parent), '-L', 'dependency=' + str(new_json.parent),
        '--extern', 'moloch_replay=' + str(new_rlib), '--extern', 'serde_json=' + str(new_json), '-o', candidate_probe])
    for role, native, rlib, serde, art, jart, sources, identity, probe, release in (
        ('baseline', old_native, old_rlib, old_json, old_art, old_json_art, old_sources, old_id, m.ROOT / 'baseline-probe', m.BASE / 'release.log'),
        ('candidate', new_native, new_rlib, new_json, new_art, new_json_art, new_sources, new_id, candidate_probe, m.ROOT / 'native-release.log')):
        found = run(role + '-identity', [probe, 'identity'], 30).read_text().strip()
        assert found == identity
        evidence['artifacts'][role] = {'probePath': str(probe), 'probe': m.pinned(probe), 'nativeRlibPath': str(rlib),
            'nativeRlib': m.pinned(rlib, artifact=True), 'nativeCargoArtifact': art, 'serdeCargoArtifact': jart,
            'nativeSourceRoot': str(native), 'sourceFiles': sources, 'engineIdentity': found,
            'optimizedProfile': 'release', 'buildEvidencePath': str(release), 'buildEvidence': m.pinned(release)}
    assert m.identity(old_native) == (old_id, old_sources) and m.identity(new_native) == (new_id, new_sources)
    evidence['probeSource'] = m.pinned(m.ROOT / 'offline_probe.rs')
    evidence['nativeTestSummary'] = {'passed': 77, 'failed': 0, 'ignored': 1, 'flowWindowTestsPassed': 5}
    evidence['finishedAt'] = dt.datetime.now(dt.timezone.utc).isoformat()
    with (ROOT / 'build-validation.json').open('x') as f:
        f.write(json.dumps(evidence, indent=2, sort_keys=True) + '\n'); f.flush(); os.fsync(f.fileno())
    print(json.dumps({'output': str(ROOT), 'candidateEngineIdentity': new_id, 'validation': m.pinned(ROOT / 'build-validation.json'),
                      'bankReplayRun': False, 'productionAuthorization': False}))

if __name__ == '__main__':
    main()
