"""Trade observations supplement books; they never manufacture executable depth."""
import bisect,gzip,hashlib,json
from pathlib import Path
import numpy as np
from moloch.learning_v2.interval_episodes import IntervalEpisode,IntervalTape
from .features import FEATURE_NAMES,Flow

FEATURE_SCHEMA='observed_conditional_intervals_and_official_prints_40_16_v5'
CONTRACT_FEATURES=32+len(FEATURE_NAMES)
GLOBAL_FEATURES=16


class FlowEpisode(IntervalEpisode):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs);self.flow=Flow()

    def _event(self,at,kind,value):
        if kind=='trade_flow':self.flow.accept(at,value)
        else:super()._event(at,kind,value)

    def observe(self):
        out=super().observe();extra=np.zeros((len(self.exchanges),len(FEATURE_NAMES)),np.float32)
        for i,ex in enumerate(self.exchanges[1:],1):extra[i]=self.flow.observe(ex,self.now)
        out['contracts']=np.concatenate((out['contracts'],extra),axis=-1)
        return out


class FlowTape(IntervalTape):
    episode_type=FlowEpisode

    def __init__(self,directory):
        directory=Path(directory);raw=(directory/'dataset-manifest.json').read_bytes();manifest=json.loads(raw)
        digest=hashlib.sha256(raw).hexdigest()
        if json.loads((directory/'COMPLETE.json').read_text())['manifestSha256']!=digest:raise ValueError('FLOW_MANIFEST_CHANGED')
        if manifest['featureSchema']!=FEATURE_SCHEMA or manifest['productionAuthority']is not False:raise ValueError('EXPLICIT_FLOW_RESEARCH_SCHEMA')
        super().__init__(Path(manifest['baseDataset']))
        if self.manifest_sha256!=manifest['baseManifestSha256']:raise ValueError('ORIGINAL_BASE_DATASET_CHANGED')
        p=directory/'trade-flow.jsonl.gz'
        if hashlib.sha256(p.read_bytes()).hexdigest()!=manifest['tradeRowsSha256']:raise ValueError('ORIGINAL_TRADE_ROWS_CHANGED')
        with gzip.open(p,'rt',encoding='utf-8')as f:self.flow_rows=tuple(json.loads(line)for line in f)
        self.flow_times=tuple(row['available_at_ms']for row in self.flow_rows)
        verifier=Flow()
        for row in self.flow_rows:verifier.accept(row['available_at_ms'],row)
        self.manifest=manifest;self.manifest_sha256=digest

    def episode(self,mechanics,start_ms,end_ms,*,initial_cash_micros,target_wealth_micros,burnin_ms=60000,advance_to_start=True):
        env=super().episode(mechanics,start_ms,end_ms,initial_cash_micros=initial_cash_micros,
            target_wealth_micros=target_wealth_micros,burnin_ms=burnin_ms,advance_to_start=False)
        lo=bisect.bisect_left(self.flow_times,start_ms-burnin_ms);hi=bisect.bisect_right(self.flow_times,end_ms)
        for row in self.flow_rows[lo:hi]:env.clock.schedule(row['available_at_ms'],10,'trade_flow',row)
        if advance_to_start:env.advance(start_ms)
        return env
