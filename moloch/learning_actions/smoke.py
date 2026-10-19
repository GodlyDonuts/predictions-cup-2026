"""GPU raw-action smoke on previously inspected history; no training or HTTP."""
import argparse,json,time,hashlib
from pathlib import Path
import torch
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_realism.protocol import Split,DAY
from moloch.learning_v2.run_experiment import load_mechanics
from .engine import ApiTape
from .policy import ApiPolicy
from .stream import Stream


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dataset',type=Path,required=True);p.add_argument('--mechanics',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--device',default='cuda')
    p.add_argument('--decisions',type=int,default=128);p.add_argument('--hidden-size',type=int,default=4096)
    args=p.parse_args()
    if args.output.exists():raise ValueError('NEW_SMOKE_RESULT_REQUIRED')
    torch.manual_seed(1902);torch.set_num_threads(2)
    original=FlowTape(args.dataset);split=Split(original.times[0],original.times[-1]+9*DAY)
    tape=ApiTape.from_loaded(original,split=split,role='train')
    start=original.times[0]+1000;end=min(original.times[-1]-1,start+60000)
    env=tape.episode(load_mechanics(args.mechanics),start,end,initial_cash_micros=100000000000,
                     target_wealth_micros=1100000000000,burnin_ms=1000)
    model=ApiPolicy(hidden_size=args.hidden_size).to(args.device)
    stream=Stream(env,model)
    # Random policy choices are actual neural samples. No profitable strategy
    # is inferred from them; the point is validating the wider running adapter.
    result=stream.run_chunk(args.decisions)
    out={'classification':'inspected_history_raw_api_neural_smoke_not_profit_or_training',
        'dataset_manifest_sha256':tape.manifest_sha256,'result':result,
        'architecture':model.architecture,'parameters':sum(v.numel()for v in model.parameters()),
        'compute_calibration':stream.calibration,'sig_read_admissions':len(env.budget.reads),
        'sig_write_admissions':len(env.budget.writes),'cash':env.cash,
        'modeled_fills':len(env.api_fills),'api_requests':len(env.idempotency),
        'trace':list(stream.rows),'budget':vars(env.budget.config),
        'http_requests':0,'production_changed':False,'independent_test_used':False,
        'full_api_parity_established':False,'profitability_established':False}
    out['source_sha256']={p.name:hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in Path(__file__).parent.glob('*.py')if not p.name.startswith('test_')}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    with args.output.open('x',encoding='utf8')as f:json.dump(out,f,indent=2,allow_nan=False)
    print(json.dumps({k:v for k,v in out.items()if k!='trace'}))


if __name__=='__main__':main()
