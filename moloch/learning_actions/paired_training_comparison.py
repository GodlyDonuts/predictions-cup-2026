"""Predeclared development comparison, no final-test or account access."""
import argparse,copy,json,time
from pathlib import Path
import torch
from .experiment import Experiment
from .stream import calibrate
from .selection import frozen
from .population_handoff import sha_file


def compare(config,output):
    if config.get('mode')!='inspected_development'or config.get('device')!='cpu':
        raise ValueError('EXPLICIT_CPU_DEVELOPMENT_COMPARISON_REQUIRED')
    if config.get('test_finalist_once',False):raise ValueError('NO_INDEPENDENT_TEST_IN_METHOD_COMPARISON')
    if config.get('development_initial_checkpoints')or config.get('development_initial_checkpoint'):
        raise ValueError('FRESH_PAIRED_FOUNDERS_REQUIRED')
    if config.get('fresh_founders')!=config['members']or config.get('cycles')!=1:
        raise ValueError('ONE_FRESH_COHORT_PER_PAIRED_FOUNDER_REQUIRED')
    output=Path(output);output.mkdir(parents=True,exist_ok=False);base=copy.deepcopy(config)
    base['closed_episode_return_replay']=False
    calibration_experiment=Experiment(base,output/'calibration-source')
    try:
        torch.manual_seed(config['seed']);model=calibration_experiment.make_policy()
        if config.get('initial_short_alarm_prior',True):calibration_experiment.initialize_alarm_prior(model)
        a,b=calibration_experiment.train_bounds
        length=config['development_episode_ms'];env=calibration_experiment.world('train',a,a+length)
        calibration=calibrate(env,model)
        plan={'classification':'paired_inspected_history_training_method_comparison_not_roi_qualification',
            'datasetManifestSha256':calibration_experiment.dataset_sha,'runtimeSourceSha256':calibration_experiment.frozen['source_sha256'],
            'commonConfig':base,'commonCalibration':calibration,'arms':['streaming','closed_episode'],
            'architecture':model.architecture,'parameters':sum(p.numel()for p in model.parameters()),
            'sharedFounderSeeds':[config['seed']+i for i in range(config['members'])],
            'originalTrainBoundsMs':list(calibration_experiment.train_bounds),
            'originalSelectionBoundsMs':list(calibration_experiment.evolution_bounds),
            'independentTestUsed':False,'productionAuthority':False}
        frozen(output/'PLAN.json',plan)
        del model,env
    finally:calibration_experiment.close()
    results=[]
    for name,closed in [('streaming',False),('closed_episode',True)]:
        arm=copy.deepcopy(base);arm['closed_episode_return_replay']=closed
        experiment=Experiment(arm,output/name);experiment.calibration=copy.deepcopy(calibration)
        frozen(experiment.output/'compute-calibration.json',calibration)
        started=time.perf_counter()
        try:
            result=experiment.run();rows=[]
            for i in range(config['members']):
                candidate=json.loads((experiment.output/('result-000-%03d.json'%i)).read_text())
                rows.append({'member':i,'checkpointSha256':candidate['checkpoint']['sha256'],
                    'worstMarkedRoi':min(c['marked_roi']for c in candidate['selection']['cases']),
                    'selectionReward':candidate['selection']['reward'],
                    'selectionFills':sum(c['fill_count']for c in candidate['selection']['cases']),
                    'trainingFills':candidate['training']['fill_count']})
            founders=[]
            for i in range(config['members']):
                p=experiment.output/('founder-%03d.pt'%i)
                bundle=torch.load(p,map_location='cpu',weights_only=True)
                founders.append(bundle['parameter_sha256']);del bundle
            results.append({'arm':name,'elapsedSeconds':time.perf_counter()-started,
                'experimentSha256':sha_file(experiment.output/'experiment.json'),
                'decisions':result['total_decisions'],'acceptedUpdates':result['total_accepted_updates'],
                'founderParameterSha256':founders,'candidates':rows,'independentTestUsed':result['independent_test_used']})
            frozen(output/(name+'-result.json'),results[-1])
        finally:experiment.close()
    if results[0]['founderParameterSha256']!=results[1]['founderParameterSha256']:
        raise ValueError('PAIRED_INITIAL_POLICY_WEIGHTS_DIFFER')
    summary={'classification':plan['classification'],'planSha256':sha_file(output/'PLAN.json'),
        'commonCalibration':calibration,'identicalFounderParameters':True,'results':results,
        'pairedWorstRoiDifferenceClosedMinusStreaming':[b['worstMarkedRoi']-a['worstMarkedRoi']for a,b in
            zip(results[0]['candidates'],results[1]['candidates'])],
        'independentTestUsed':False,'productionChanged':False,'fullSizeGpuQualified':False,
        'independentProfitableRoiEstablished':False}
    frozen(output/'COMPARISON.json',summary);return summary


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--config',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    result=compare(json.loads(args.config.read_text()),args.output)
    print(json.dumps({'completed':True,'comparison':str(args.output/'COMPARISON.json')}),flush=True)
