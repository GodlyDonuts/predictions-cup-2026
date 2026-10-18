"""Finite development stress review and pre-window checkpoint/source freeze.

This grants no production authority. Missing/losing stress cases fail; zero
returns from an unobservable calendar cannot qualify a candidate.
"""
import argparse
from dataclasses import replace
from datetime import datetime,timezone
import hashlib
import json
from pathlib import Path
import shutil
import torch
from .episodes import Tape
from .policy import RecurrentPolicy
from .run_experiment import load_mechanics,rollout,sha,write_json
from .train_inner import fingerprint


def main():
    p=argparse.ArgumentParser();p.add_argument('--population',type=Path,required=True);p.add_argument('--dataset',type=Path,required=True)
    p.add_argument('--mechanics',type=Path,required=True);p.add_argument('--base-protocol',type=Path,required=True)
    p.add_argument('--forward-protocol',type=Path,required=True);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    if args.output.exists():raise ValueError('IMMUTABLE_NEW_FREEZE_REQUIRED')
    args.output.mkdir(parents=True);torch.set_num_threads(4)
    forward=json.loads(args.forward_protocol.read_text());deadline=datetime.fromisoformat(forward['freeze_deadline_utc'].replace('Z','+00:00'))
    plan=json.loads((args.population/'protocol.json').read_text());split=sha(args.population/'protocol.json')
    if plan['base_protocol_sha256']!=sha(args.base_protocol)or plan['mechanics_manifest_sha256']!=sha(args.mechanics):raise ValueError('PARENT_EXPERIMENT_ROLE_OR_MECHANICS_CHANGED')
    latest={}
    for path in sorted(args.population.glob('c[0-9]*-s[0-9]*.json')):
        r=json.loads(path.read_text());key=r['slot']
        if key not in latest or r['cycle']>latest[key]['cycle']:latest[key]=r
    candidates=[r for r in latest.values()if all(e['qualified_accounting']and e.get('observable_market_frames',0)>0 and e['reference_return']is not None and e['reference_return']>0 for e in r['search_evaluations'])]
    candidates.sort(key=lambda r:min(e['reference_return']for e in r['search_evaluations']),reverse=True)
    write_json(args.output/'review-plan.json',{'parent_protocol_sha256':split,'candidate_hashes_before_review':[r['checkpoint_sha256']for r in candidates[:4]],
                'selection':'strict_positive_both_nominal_blocks_then_joint_stress','maximum_review_candidates':4,
                'stress':{'read_latency_multiplier':1.5,'arrival_latency_multiplier':2,'receipt_latency_multiplier':2,'depth_fraction_ppm':500000,'adverse_execution_micros':5000},
                'forward_protocol_sha256':sha(args.forward_protocol),'production_authority':False})
    tape=Tape(args.dataset);base=load_mechanics(args.mechanics)
    protocol={**plan['base_protocol'],'_sha256':split,'episode_length_ms':plan['search_episode_length_ms']}
    stress=replace(base,read_latency_ms=base.read_latency_ms*3//2,arrival_latency_ms=base.arrival_latency_ms*2,
                   receipt_latency_ms=base.receipt_latency_ms*2,depth_fraction_ppm=500000,adverse_execution_micros=5000)
    reviews=[];selected=[];founders=set()
    for r in candidates[:4]:
        if len(selected)>=forward['finalists_count']or datetime.now(timezone.utc)>=deadline:break
        if r['founder']in founders:continue
        name=f'c{r["cycle"]:03d}-s{r["slot"]:03d}-{r["id"]}.pt';cp=args.population/'checkpoints'/name
        if cp.resolve().parent!=(args.population/'checkpoints').resolve()or sha(cp)!=r['checkpoint_sha256']:raise ValueError('CHECKPOINT_SOURCE_CHANGED')
        bundle=torch.load(cp,map_location='cuda',weights_only=True)
        if bundle['production_authority']is not False or bundle['split_manifest_sha256']!=split:raise ValueError('CHECKPOINT_AUTHORITY_OR_ROLE')
        model=RecurrentPolicy(bundle['contract_features'],bundle['global_features'],bundle['hidden_size']).cuda();model.load_state_dict(bundle['model'])
        weights=fingerprint(model);out=[]
        for label,mechanics in [('nominal',base),('joint_adverse',stress)]:
            for j,start in enumerate(plan['search_starts_ms']):
                torch.manual_seed(plan['seed']+900000+j)
                t,e=rollout(tape,mechanics,protocol,model,start,split_role='search_development');del t
                e.update(profile=label,parameter_sha256=weights);out.append(e)
        qualified=all(e['qualified_accounting']and e['observable_market_frames']>0 and e['reference_return']is not None and e['reference_return']>0 for e in out)
        item={'id':r['id'],'founder':r['founder'],'checkpoint_sha256':r['checkpoint_sha256'],'parameter_sha256':weights,'evaluations':out,
              'qualified_under_declared_development_stress':qualified,'production_authority':False}
        reviews.append(item);write_json(args.output/f'review-{len(reviews):02d}.json',item)
        if qualified and datetime.now(timezone.utc)<deadline:
            dst=args.output/f'finalist-{len(selected)+1}.pt';shutil.copyfile(cp,dst)
            if sha(dst)!=r['checkpoint_sha256']:raise ValueError('FROZEN_CHECKPOINT_COPY')
            selected.append({'id':r['id'],'founder':r['founder'],'checkpoint_file':dst.name,'checkpoint_sha256':r['checkpoint_sha256'],
                             'parameter_sha256':weights,'contract_features':model.contract_features,'global_features':model.global_features})
            founders.add(r['founder'])
        del model
    frozen_at=datetime.now(timezone.utc);source_dir=args.output/'source/moloch/learning_v2';source_dir.mkdir(parents=True)
    hashes={}
    for source in Path(__file__).parent.glob('*.py'):
        raw=source.read_bytes();(source_dir/source.name).write_bytes(raw);hashes[source.name]=hashlib.sha256(raw).hexdigest()
    result={'classification':'pre_window_conditional_research_finalists_not_live_approval','frozen_at_utc':frozen_at.isoformat(),
            'before_deadline':frozen_at<deadline,'requested_finalists':forward['finalists_count'],'qualified_finalists':selected,
            'required_count_met':len(selected)==forward['finalists_count']and frozen_at<deadline,
            'reviews':reviews,'parent_protocol_sha256':split,'forward_protocol_sha256':sha(args.forward_protocol),
            'frozen_source_sha256':hashes,'nominal_and_stress_are_development_selection':True,'locked_or_forward_outcomes_used':False,
            'competition_profitability_established':False,'production_authority':False}
    write_json(args.output/'finalists.json',result);print(json.dumps({'qualified':len(selected),'required_count_met':result['required_count_met'],'frozen_at':result['frozen_at_utc']}),flush=True)


if __name__=='__main__':main()
