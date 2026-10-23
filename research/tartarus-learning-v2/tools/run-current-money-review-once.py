"""Publish one fresh ranked money review after exact original-history recovery.

Uses the existing scanner's best prepared candidate rather than a fixed market.
No account client, deadline renewal, recurring loop or old-intent retry.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import time
from review_generation_window import publication_boundary


def checked_recovery(deployment, proof, progress, process_exists):
    assert process_exists, 'EXPECTED_WORKER_EXITED'
    if proof is None:
        return False
    assert proof['pid'] == deployment['pid'], 'RESTORATION_PROOF_WORKER_CHANGED'
    assert proof['freshTradingVerified'] is True
    assert proof['fullOriginalFinancialPrefixPreserved'] is True
    assert proof['fullOriginalFundamentalPrefixPreserved'] is True
    assert proof['originalFillPrefixPreserved'] is True
    return (progress.get('pid') == deployment['pid'] and progress.get('phase') == 'trading'
            and progress.get('realtimeHealthy') is True)


def research_command(template, deployment_path, output):
    command = list(template)
    command[command.index('--deployment')+1] = str(deployment_path)
    command[command.index('--output')+1] = str(output)
    # Do not silently broaden the original finite-horizon price assumptions.
    assert command[command.index('--decision-profile-policy')+1] == 'current-prices'
    return command


def run(a):
    os.umask(0o077); os.nice(15); Path('/proc/self/oom_score_adj').write_text('1000')
    assert len(a.source_commit) == 40 and all(c in '0123456789abcdef' for c in a.source_commit)
    a.output.mkdir(mode=0o700,parents=True,exist_ok=False)
    deployment = json.loads(a.deployment.read_bytes())
    process = Path('/proc')/str(deployment['pid'])

    def record(**fields):
        fields.update(accountClientsCreated=0,ordersSubmittedByController=0,fillClaimed=False,
                      sourceClocksRenewed=False,sourceCommit=a.source_commit)
        (a.output/'result.public.json').write_text(json.dumps(fields,indent=2)+'\n')
        print(json.dumps(fields),flush=True)

    end = time.monotonic()+a.wait_seconds
    while True:
        assert not (a.runtime/'EXECUTION_STOP').exists(), 'EXECUTION_STOP_PRESENT'
        proof = json.loads(a.restoration_proof.read_bytes()) if a.restoration_proof.exists() else None
        progress = json.loads((a.runtime/'progress.json').read_bytes())
        if checked_recovery(deployment,proof,progress,process.exists()):
            assert hashlib.sha256((process/'exe').read_bytes()).hexdigest() == deployment['binarySha256']
            assert int((process/'stat').read_text().rsplit(') ',1)[1].split()[19]) == deployment['startTicks']
            break
        if time.monotonic() >= end:
            record(state='finite_restoration_wait_expired'); return
        time.sleep(5)
    with publication_boundary(a.runtime,wait_seconds=240) as fd:
        deadline = time.monotonic()+300
        while json.loads((a.reference/'pending.json').read_bytes()).get('resolved') is not True:
            if time.monotonic() >= deadline:
                record(state='source_reload_requires_reconciliation'); return
            time.sleep(.2)

        def call(stage, command):
            remaining=deadline-time.monotonic()
            if remaining <= 0:
                record(state='bounded_review_window_expired_before_launch',stage=stage,
                       publicationOutcomeRequiresReconciliation=False)
                raise RuntimeError('BOUNDED_REVIEW_WINDOW_EXPIRED_BEFORE_LAUNCH')
            try:
                result = subprocess.run(command,capture_output=True,text=True,
                    timeout=remaining,pass_fds=(fd,) if stage=='publish' else ())
            except subprocess.TimeoutExpired:
                record(state='bounded_review_window_expired',stage=stage,
                       publicationOutcomeRequiresReconciliation=stage=='publish')
                raise
            (a.output/(stage+'.log')).write_text(result.stdout+result.stderr)
            print(stage,result.returncode,flush=True)
            return result

        command = research_command(json.loads(a.research_command.read_bytes()),a.deployment,a.output/'research')
        result = call('research',command)
        if result.returncode:
            record(state='fresh_research_declined',diagnosticTail=result.stderr[-700:]); return
        research = json.loads((a.output/'research/result.public.json').read_bytes())
        if research['state'] != 'fresh_unapproved_nomination_prepared':
            record(state=research['state']); return
        prepared = a.output/'research/prepared'
        summary = json.loads((prepared/'prepared.public.json').read_bytes())
        assert summary['weakestDeclaredExpectedChange'] > 0
        assert summary['sourceClocksRenewed'] is False
        assert time.time_ns()//1000000 < summary['validUntilMs'], 'ORIGINAL_REVIEW_EXPIRED'
        assert hashlib.sha256(a.publisher.read_bytes()).hexdigest() == a.publisher_sha256
        result = call('publish',['python3',str(a.publisher),'--runtime',str(a.runtime),
            '--prepared',str(prepared),'--expected-pid',str(deployment['pid']),
            '--expected-binary-sha256',deployment['binarySha256'],'--expected-raw-sha256',summary['rawSha256'],
            '--source-commit',a.source_commit,'--source-reference-root',str(a.reference),
            '--generation-publication-lock-fd',str(fd)])
        record(state='owner_review_published' if result.returncode==0 else 'publication_declined',
               nomination=summary,publisherExitCode=result.returncode,
               diagnosticTail=result.stderr[-1000:] if result.returncode else None,
               workerIndependentlyReconcilesAndDecides=True,retryOfPriorIntentAuthorized=False)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['runtime','reference','deployment','restoration-proof','research-command','publisher','output']:
        p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--publisher-sha256',required=True)
    p.add_argument('--source-commit',required=True)
    p.add_argument('--wait-seconds',type=int,default=2700)
    run(p.parse_args())
