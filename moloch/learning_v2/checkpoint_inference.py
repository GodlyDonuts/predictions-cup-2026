"""Frozen inference only: verify original bytes, semantics and actual weights.

Market recurrence shares its weights across slots. A new universe can use a
different slot count with fresh zero memory, without changing learned weights.
This neither trains a policy nor grants live execution authority.
"""
import hashlib
import io
from pathlib import Path
import torch
from .market_memory_policy import MarketMemoryPolicy
from .interval_market_policy import IntervalMarketPolicy
from .train_inner import fingerprint

SEMANTIC_SOURCES=('episodes.py','reference_env.py','account.py','liquidity.py',
                  'policy.py','fused_policy.py','market_memory_policy.py','run_experiment.py')


def load_frozen_policy(path,expected_sha256,*,device='cpu',market_slots=None):
    raw=Path(path).read_bytes()
    if hashlib.sha256(raw).hexdigest()!=expected_sha256:raise ValueError('CHECKPOINT_BYTES_CHANGED')
    bundle=torch.load(io.BytesIO(raw),map_location='cpu',weights_only=True)
    interval=bundle['architecture']==IntervalMarketPolicy.architecture
    if bundle['architecture']not in [MarketMemoryPolicy.architecture,IntervalMarketPolicy.architecture] or bundle['production_authority']is not False:
        raise ValueError('RESEARCH_MARKET_MEMORY_CHECKPOINT_REQUIRED')
    if (bundle['contract_features'],bundle['global_features'])!=((32,16)if interval else(24,14)):raise ValueError('CHECKPOINT_FEATURE_SEMANTICS')
    provenance=bundle['provenance'];shape=provenance['architecture_shape']
    sources=SEMANTIC_SOURCES+('interval_market_policy.py','interval_episodes.py','interval_excess_reward.py')if interval else SEMANTIC_SOURCES
    for name in sources:
        if hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest()!=provenance['source_sha256'].get(name):
            raise ValueError('CHECKPOINT_SEMANTIC_SOURCE_CHANGED:'+name)
    original_slots=shape['market_slots'];slots=original_slots if market_slots is None else market_slots
    if type(slots)is not int or slots<=0:raise ValueError('POSITIVE_MARKET_SLOTS_REQUIRED')
    if shape['actor_state_size']!=shape['global_hidden_size']+original_slots*(shape['market_hidden_size']+1):
        raise ValueError('CHECKPOINT_ORIGINAL_ACTOR_STATE_SHAPE')
    if bundle['hidden_size']!=shape['actor_state_size']:raise ValueError('CHECKPOINT_PACKED_STATE_SHAPE')
    model=(IntervalMarketPolicy if interval else MarketMemoryPolicy)(slots,hidden_size=shape['global_hidden_size'],market_hidden_size=shape['market_hidden_size'])
    model.load_state_dict(bundle['model'],strict=True)
    if fingerprint(model)!=bundle['parameter_sha256']:raise ValueError('CHECKPOINT_PARAMETER_FINGERPRINT')
    model.requires_grad_(False);model.eval();model.to(device)
    return model,{'checkpoint_sha256':expected_sha256,'parameter_sha256':bundle['parameter_sha256'],
                  'split_manifest_sha256':bundle['split_manifest_sha256'],
                  'original_market_slots':original_slots,'inference_market_slots':slots,
                  'fresh_actor_state_required':True,'optimizer_restored':False,'production_authority':False}
