"""Recurrent PPO on fresh simulator trajectories, with immutable data roles.

Complete episodes start with their own recurrent reset. Checkpoint/mutation
transfer invalidates old trajectories by a full parameter fingerprint. Market
and execution assumptions are not trainer parameters.
"""
from dataclasses import dataclass, asdict
import hashlib
import math
from pathlib import Path
import torch
from .policy import RecurrentPolicy, ARCHITECTURE


@dataclass(frozen=True)
class PPOConfig:
    learning_rate: float = 3e-4
    entropy_coefficient: float = 0.01
    clip_ratio: float = 0.2
    epochs: int = 4
    gamma_per_second: float = 0.999
    lambda_per_second: float = 0.95
    value_coefficient: float = 0.5
    quantile_coefficient: float = 0.1
    success_coefficient: float = 0.1
    maximum_gradient_norm: float = 0.5

    def __post_init__(self):
        if not (0 < self.learning_rate < 1 and 0 < self.clip_ratio < 1 and
                0 < self.gamma_per_second <= 1 and 0 < self.lambda_per_second <= 1 and
                type(self.epochs) is int and self.epochs > 0 and self.maximum_gradient_norm > 0):
            raise ValueError('PPO_CONFIGURATION')
        if any(not math.isfinite(v) for v in asdict(self).values()):
            raise ValueError('NONFINITE_PPO_CONFIGURATION')


def fingerprint(model):
    h = hashlib.sha256()
    for name, value in sorted(model.state_dict().items()):
        h.update(name.encode()); h.update(str(tuple(value.shape)).encode())
        h.update(value.detach().cpu().contiguous().numpy().tobytes())
    return h.hexdigest()


@dataclass
class Trajectory:
    contracts: torch.Tensor
    global_state: torch.Tensor
    present: torch.Tensor
    masks: dict
    reset: torch.Tensor
    actions: dict
    old_logprob: torch.Tensor
    old_values: torch.Tensor
    rewards: torch.Tensor
    elapsed_ms: torch.Tensor
    done: torch.Tensor
    learn: torch.Tensor
    target_success_labels: torch.Tensor  # NaN means no observed/declared label.
    behavior_sha256: str
    split_manifest_sha256: str
    split_role: str


def targets(t, config):
    """Time-aware GAE plus a separate undiscounted monetary return target."""
    advantages = torch.zeros_like(t.rewards)
    monetary = torch.zeros_like(t.rewards)
    ga = torch.zeros_like(t.rewards[0]); money = torch.zeros_like(ga)
    for i in reversed(range(t.rewards.shape[0])):
        continuation = (~t.done[i]).to(t.rewards.dtype)
        next_value = t.old_values[i+1] if i+1 < t.rewards.shape[0] else torch.zeros_like(ga)
        gamma = torch.exp(t.elapsed_ms[i]/1000 * math.log(config.gamma_per_second))
        lam = torch.exp(t.elapsed_ms[i]/1000 * math.log(config.lambda_per_second))
        delta = t.rewards[i] + gamma*next_value*continuation - t.old_values[i]
        ga = delta + gamma*lam*continuation*ga
        money = t.rewards[i] + continuation*money
        advantages[i] = ga; monetary[i] = money
    return advantages, advantages+t.old_values, monetary


class PPOTrainer:
    def __init__(self, model: RecurrentPolicy, config: PPOConfig, split_manifest_sha256: str):
        if len(split_manifest_sha256) != 64:
            raise ValueError('PINNED_SPLIT_REQUIRED')
        self.model = model; self.config = config; self.split = split_manifest_sha256
        self.optimizer = torch.optim.Adam(model.parameters(), lr=config.learning_rate)
        self.policy_epoch = 0
        self.training_decisions = 0

    def update(self, t: Trajectory):
        if t.split_role != 'fit_training' or t.split_manifest_sha256 != self.split:
            raise ValueError('TRAINING_ROLE_OR_SPLIT_MISMATCH')
        if t.behavior_sha256 != fingerprint(self.model):
            raise ValueError('MUTATED_OR_REPLACED_POLICY_REQUIRES_FRESH_TRAJECTORIES')
        if not bool(t.reset[0].all()) or not bool(t.done[-1].all()):
            raise ValueError('COMPLETE_EPISODE_RECURRENT_BOUNDARIES_REQUIRED')
        if not bool((t.elapsed_ms > 0).all()) or not bool(t.learn.any()):
            raise ValueError('TIME_OR_LEARNING_MASK')
        if any(x.requires_grad for x in [t.old_logprob,t.old_values,t.rewards]):
            raise ValueError('BEHAVIOR_TARGETS_MUST_BE_DETACHED')
        if any(not bool(torch.isfinite(x).all()) for x in [t.old_logprob,t.old_values,t.rewards,t.elapsed_ms]):
            raise ValueError('NONFINITE_TRAJECTORY')
        with torch.no_grad():
            advantage, returns, money = targets(t,self.config)
            selected = advantage[t.learn]
            advantage = (advantage-selected.mean()) / selected.std(unbiased=False).clamp_min(1e-8)
        results = []
        for epoch in range(self.config.epochs):
            rows = self.model.sequence(t.contracts,t.global_state,t.present,t.masks,t.reset,t.actions)
            logprob = torch.stack([r['logprob'] for r in rows])
            values = torch.stack([r['value'] for r in rows])
            entropy = torch.stack([r['entropy'] for r in rows])
            quantiles = torch.stack([r['monetary_quantiles'] for r in rows])
            success = torch.stack([r['success_logit'] for r in rows])
            ratio = (logprob-t.old_logprob).exp()
            surrogate = torch.minimum(ratio*advantage, ratio.clamp(1-self.config.clip_ratio,1+self.config.clip_ratio)*advantage)
            policy_loss = -surrogate[t.learn].mean()
            value_loss = (values[t.learn]-returns[t.learn]).square().mean()
            errors = money.unsqueeze(-1)-quantiles
            q = torch.arange(1,10,device=errors.device,dtype=errors.dtype)/10
            quantile_loss = (torch.maximum(q*errors,(q-1)*errors))[t.learn].mean()
            valid_success = torch.isfinite(t.target_success_labels) & t.learn
            success_loss = (torch.nn.functional.binary_cross_entropy_with_logits(success[valid_success],t.target_success_labels[valid_success])
                            if bool(valid_success.any()) else success.sum()*0)
            loss = (policy_loss+self.config.value_coefficient*value_loss+self.config.quantile_coefficient*quantile_loss
                    +self.config.success_coefficient*success_loss-self.config.entropy_coefficient*entropy[t.learn].mean())
            if not bool(torch.isfinite(loss)):
                raise ValueError('NONFINITE_PPO_LOSS')
            self.optimizer.zero_grad(set_to_none=True); loss.backward()
            norm = torch.nn.utils.clip_grad_norm_(self.model.parameters(),self.config.maximum_gradient_norm,error_if_nonfinite=True)
            self.optimizer.step()
            results.append({'epoch':epoch,'loss':float(loss.detach()),'policyLoss':float(policy_loss.detach()),
                            'valueLoss':float(value_loss.detach()),'quantileLoss':float(quantile_loss.detach()),
                            'successLabels':int(valid_success.sum()),'gradientNormBeforeClip':float(norm),
                            'sampledApproximateKl':float((t.old_logprob-logprob).detach()[t.learn].mean())})
        self.policy_epoch += 1; self.training_decisions += int(t.learn.sum())
        return results

    def save(self, path: Path, provenance: dict):
        required = ['dataset_manifest_sha256','mechanics_manifest_sha256','evaluation_protocol_sha256',
                    'action_adapter_sha256','lineage','classification']
        if any(not provenance.get(k) for k in required):
            raise ValueError('CHECKPOINT_PROVENANCE_INCOMPLETE')
        if path.exists(): raise ValueError('IMMUTABLE_CHECKPOINT_EXISTS')
        bundle = {'architecture':ARCHITECTURE,'contract_features':self.model.contract_features,
                  'global_features':self.model.global_features,'hidden_size':self.model.hidden_size,
                  'model':self.model.state_dict(),'optimizer':self.optimizer.state_dict(),
                  'config':asdict(self.config),'policy_epoch':self.policy_epoch,'training_decisions':self.training_decisions,
                  'split_manifest_sha256':self.split,'parameter_sha256':fingerprint(self.model),
                  'provenance':provenance,'torch_version':str(torch.__version__),
                  'production_authority':False}
        with path.open('xb') as output: torch.save(bundle,output)
        return hashlib.sha256(path.read_bytes()).hexdigest()
