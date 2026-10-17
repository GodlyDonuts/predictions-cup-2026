"""Reproduce the fixed earlier-year joint diagnostic; no live model or client."""
import os
os.environ.setdefault('OPENBLAS_NUM_THREADS', '1')
os.environ.setdefault('OMP_NUM_THREADS', '1')
import argparse, datetime, hashlib, json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import model

TRAINING_SHA = '96c140d777bf70d020ddafb6091c684c72676f1819543f00b85263864645d465'
OLD_VALIDATION_SHA = '8e0af78b39d4f17ca6872b32111328c6e46e206945fca9c91bb4336ba8c090fb'
SOURCE_SHA = '6da558523a212ae826d2580400944be581046f7f04150d7336ad773b72c79232'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--training', type=Path, required=True)
    p.add_argument('--old-validation', type=Path, required=True)
    p.add_argument('--old-model', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    a = p.parse_args()
    if a.out.exists():
        raise ValueError('new output directory required; originals are immutable')
    assert sha(a.training) == TRAINING_SHA
    assert sha(a.old_validation) == OLD_VALIDATION_SHA
    source = Path(model.__file__).read_bytes().replace(b'\r\n', b'\n')
    assert hashlib.sha256(source).hexdigest() == SOURCE_SHA
    training = json.loads(a.training.read_bytes())
    old = json.loads(a.old_validation.read_bytes())
    fitted = model.fit(training['rows'])
    assert fitted == json.loads(a.old_model.read_bytes())
    result = model.validate(training['rows'])
    for key, value in old.items():
        assert result[key] == value, key
    assert all(f['trainedThroughYear'] < f['heldOutYear'] for f in result['chronologicalJointFolds'])
    assert len(result['chronologicalJointPairRows']) == 5640
    summary = {
        'observedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'trainingSha256': TRAINING_SHA, 'originalValidationSha256': OLD_VALIDATION_SHA,
        'originalModelSha256': sha(a.old_model), 'sourceGitLfSha256': SOURCE_SHA,
        'allOriginalValidationFieldsExactlyReproduced': sorted(old),
        'originalFittedModelExactlyReproduced': True,
        'chronologicalMarginals': result['chronologicalOutcomeDiagnostic'],
        'chronologicalJointFolds': result['chronologicalJointFolds'],
        'chronologicalJointCycleBootstrap': result['chronologicalJointCycleBootstrap'],
        'limitations': result['limitations'], 'sourceReleaseClocksKnown': False,
        'currentForecastOrControlChanged': False, 'modelApproval': False,
        'executionAuthority': False, 'profitEstablished': False,
    }
    a.out.mkdir(parents=True)
    for name, value in [('validation.json', result), ('summary.json', summary)]:
        with (a.out / name).open('x', encoding='utf-8') as f:
            json.dump(value, f, indent=2, sort_keys=True)
            f.write('\n')
    print(json.dumps({'oldFieldsExact': True, 'oldModelExact': True,
                      **result['chronologicalJointCycleBootstrap']}))


if __name__ == '__main__':
    main()
