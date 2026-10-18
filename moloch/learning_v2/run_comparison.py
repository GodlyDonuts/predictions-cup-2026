"""Finite shared-calendar comparison. No HTTP, credentials or live adapter.

This matches scheduled policy-training/evaluation work, not exact floating-point
operations or elapsed time: mutation and replacement add measured overhead.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time
from .run_experiment import sha, write_json


def main():
    p = argparse.ArgumentParser()
    for flag in ['dataset', 'mechanics', 'protocol', 'execution-profiles', 'output']:
        p.add_argument('--'+flag, type=Path, required=True)
    p.add_argument('--members', type=int, default=4)
    p.add_argument('--islands', type=int, default=1)
    p.add_argument('--cycles', type=int, default=3)
    p.add_argument('--founder-grace-blocks', type=int, default=2)
    p.add_argument('--seed', type=int, default=10001)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    modes = ['independent', 'local', 'multiscale']
    plan = {'classification':'scheduled_work_matched_development_comparison_not_locked_evidence',
            'modes':modes, 'members':args.members, 'islands':args.islands, 'cycles':args.cycles,
            'seed':args.seed, 'initial_founder_grace_training_blocks':args.founder_grace_blocks,
            'dataset_sha256':sha(args.dataset/'dataset-manifest.json'),
            'mechanics_sha256':sha(args.mechanics), 'protocol_sha256':sha(args.protocol),
            'execution_profiles_sha256':sha(args.execution_profiles),
            'source_sha256':{x.name:sha(x) for x in sorted(Path(__file__).parent.glob('*.py')) if not x.name.startswith('test_')},
            'matching':'same founders, calendars, scheduled training decisions, bankrolls, evaluation profiles and seeds',
            'unmatched':'replacement/mutation/probe overhead and rejected optimizer steps; measure actual work and wall time',
            'archive':'same observational archive in all modes; archive reintroduction is not implemented',
            'production_authority':False}
    write_json(args.output/'plan.json', plan)
    begun = time.perf_counter(); results = []
    for mode in modes:
        # Fail if source changed between modes: later modes may not benefit
        # from adaptive code changes after inspecting earlier outcomes.
        if any(sha(Path(__file__).parent/name)!=digest for name,digest in plan['source_sha256'].items()):
            raise ValueError('COMPARISON_SOURCE_CHANGED')
        command = [sys.executable, '-u', '-m', 'moloch.learning_v2.run_population']
        for key in ['dataset','mechanics','protocol','execution_profiles','members','islands','cycles','seed','founder_grace_blocks']:
            command += ['--'+key.replace('_','-'), str(getattr(args,key))]
        command += ['--output', str(args.output/mode), '--replacement-mode', mode]
        start = time.perf_counter()
        with (args.output/(mode+'.log')).open('xb') as log:
            process = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
        if process.returncode:
            raise RuntimeError('COMPARISON_CHILD_FAILED:'+mode)
        folder = args.output/mode
        report = json.loads((folder/'report.json').read_text(encoding='utf-8'))
        complete = json.loads((folder/'COMPLETE.json').read_text(encoding='utf-8'))
        if sha(folder/'report.json') != complete['report_sha256']:
            raise ValueError('COMPARISON_INCOMPLETE_REPORT')
        history = report['history']
        final = [r for r in history if r['cycle']==args.cycles-1]
        result = {'mode':mode, 'report_sha256':complete['report_sha256'],
                  'wall_seconds':time.perf_counter()-start,
                  'training_decisions':sum(r['fit']['decisions'] for r in history),
                  'evaluation_decisions':sum(e['decisions'] for r in history for e in r['search_evaluations']),
                  'accepted_optimizer_updates_cumulative_at_final_slots':sum(r['accepted_optimizer_updates'] for r in final),
                  'replacement_events':len(report['genealogy']),
                  'final_worst_profile_returns':[r['selection_score'] for r in final],
                  'final_strict_positive_all_profiles':sum(r['selection_score'] is not None and r['selection_score']>0 for r in final),
                  'archive_entries':len(report['archive']['entries']),
                  'actual_returns_are_conditional_marked_development_not_ROI':True}
        results.append(result); write_json(args.output/(mode+'-summary.json'), result)
        print(json.dumps(result), flush=True)
    report = {'plan_sha256':sha(args.output/'plan.json'), 'results':results,
              'wall_seconds':time.perf_counter()-begun, 'independent_seed_repetitions':1,
              'locked_test_used':False, 'competition_profitability_established':False,
              'full_A0_A6_ablations_complete':False, 'production_changed':False}
    write_json(args.output/'report.json',report)
    write_json(args.output/'COMPLETE.json',{'report_sha256':sha(args.output/'report.json')})


if __name__=='__main__':
    main()
