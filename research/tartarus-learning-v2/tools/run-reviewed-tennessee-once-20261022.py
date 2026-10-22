"""One preselected, fresh TN money review; never a repeat or an account client."""
from pathlib import Path
import argparse
import json
import os
import subprocess
from types import SimpleNamespace
import time

ROOT = Path('/tmp/moloch-generation-pins-r103-20261022')
COMPLETION = Path('/tmp/moloch-review-completion-010-20261022')
RELEASE = Path('/home/archon/apps/moloch/releases/review-completion-011')
TOOLS = Path('/tmp/moloch-upgrade-bound-r100-20261022/money-tools')


def main(run_number):
    os.umask(0o077); os.nice(15); Path('/proc/self/oom_score_adj').write_text('1000')
    assert 0 <= run_number <= 999999
    run = ROOT/('tennessee-owner-' + format(run_number,'03d')); run.mkdir(mode=0o700)
    def record(value):
        (run/'result.public.json').write_text(json.dumps(value,indent=2)+'\n')
        print(json.dumps(value),flush=True)
    def call(name, command):
        result = subprocess.run(command,capture_output=True,text=True)
        (run/(name+'.log')).write_text(result.stdout+result.stderr)
        print(name,result.returncode,flush=True)
        return result
    command = json.loads((ROOT/'fresh-money-research-006.command.json').read_bytes())
    command[command.index('--output')+1] = str(run/'research')
    result = call('research',command)
    if result.returncode:
        record(dict(state='research_declined',ordersSubmittedByController=0)); return
    inputs = run/'research/inputs'; candidates = inputs/'direct-opportunities.public.json'
    rows = json.loads(candidates.read_bytes())['candidates']
    if not any(x['leg']['ex']=='1051' and x['leg']['side']=='NO' and x['leg']['action']=='BUY' for x in rows):
        record(dict(state='no_current_positive_tennessee_candidate',ordersSubmittedByController=0)); return
    prepared = run/'prepared'
    result = call('prepare',['python3',str(TOOLS/'prepare-current-money-nomination.py'),
        '--source','/tmp/moloch-rank-current-r99-20261022','--inputs',str(inputs),'--candidates',str(candidates),
        '--output',str(prepared),'--exchange','1051','--side','NO','--action','BUY',
        '--decision-ms',str(time.time_ns()//1000000),'--horizon-ms','300000',
        '--minimum-expected-money','0.010000','--price-tolerance','0.010000'])
    if result.returncode:
        record(dict(state='funded_positive_review_declined',ordersSubmittedByController=0)); return
    deadline = time.monotonic()+120
    while not (COMPLETION/'approved-source-commit-011.txt').exists():
        if time.monotonic() >= deadline:
            record(dict(state='checked_source_publication_missing',ordersSubmittedByController=0)); return
        time.sleep(1)
    source = (COMPLETION/'approved-source-commit-011.txt').read_text().strip()
    deployment = json.loads((ROOT/'deployment.public.json').read_bytes())
    qualification = json.loads((COMPLETION/'qualified.public.json').read_bytes())
    summary = json.loads((prepared/'prepared.public.json').read_bytes())
    result = call('publish',['python3',str(RELEASE/'publish-current-money-review.py'),
        '--runtime','/home/archon/apps/moloch/shared/live-production-v1','--prepared',str(prepared),
        '--expected-pid',str(deployment['pid']),'--expected-binary-sha256',deployment['binarySha256'],
        '--expected-raw-sha256',summary['rawSha256'],'--source-commit',source,'--completed-qid','23142',
        '--source-reference-root','/home/archon/apps/moloch/shared/fundamental-live-reference-20261021',
        '--prior-not-sent-verifier',str(RELEASE/'verify-prior-not-sent-review'),
        '--prior-not-sent-verifier-sha256',qualification['verifierBinarySha256']])
    record(dict(state='owner_review_published' if result.returncode==0 else 'publication_declined',
        nomination=summary,publisherExitCode=result.returncode,sourceCommit=source,
        diagnosticTail=result.stderr[-700:] if result.returncode else None,
        ordersSubmittedByController=0,workerIndependentlyReconcilesAndDecides=True,
        fillClaimed=False,retryOfPriorIntentAuthorized=False))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-number',type=int,required=True)
    main(parser.parse_args().run_number)
