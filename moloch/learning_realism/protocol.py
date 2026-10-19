"""Frozen chronological roles and the human's daily-growth selection score."""
from dataclasses import dataclass, asdict
import hashlib
import json
import math
import statistics

DAY = 86_400_000


@dataclass(frozen=True)
class Split:
    competition_start_ms: int
    cutoff_ms: int

    def __post_init__(self):
        if any(type(x) is not int or x < 0 for x in asdict(self).values()) or self.cutoff_ms-self.competition_start_ms <= 8*DAY:
            raise ValueError('COMPETITION_HISTORY_MUST_EXCEED_EIGHT_DAYS')

    def bounds(self, role):
        a, b = self.cutoff_ms-8*DAY, self.cutoff_ms-4*DAY
        return {'train': (self.competition_start_ms, a), 'evolution': (a, b), 'test': (b, self.cutoff_ms)}[role]

    def require(self, role, first_ms, terminal_ms, *, frozen_finalist_sha256=None):
        lo, hi = self.bounds(role)
        if not lo <= first_ms < terminal_ms < hi:
            raise ValueError('BURNIN_DECISIONS_AND_RECEIPT_DRAIN_MUST_STAY_INSIDE_ROLE')
        if role == 'test' and not valid_sha(frozen_finalist_sha256):
            raise ValueError('TEST_REQUIRES_FROZEN_FINALIST_SHA256')


def valid_sha(value):
    return isinstance(value, str) and len(value) == 64 and all(c in '0123456789abcdef' for c in value)


def independent_test_claim(manifest,split):
    """Positive source claim required; mere clock coverage is not independence.

    The producer's immutable provenance must additionally be supported by the
    local data-use audit. A Boolean flag alone cannot establish an unseen test.
    """
    claim=manifest.get('independent_test',{})
    return (claim.get('bounds_ms')==list(split.bounds('test'))
        and claim.get('prices_used_for_training_or_selection')is False
        and claim.get('uninspected_test_designation')is True
        and valid_sha(claim.get('source_evidence_sha256')))


@dataclass(frozen=True)
class Delivery:
    # Conservative configured request ceilings, not newly discovered API grants.
    kalshi_reads_per_second: int = 20
    polymarket_reads_per_second: int = 30
    external_latency_ms: int = 150

    def __post_init__(self):
        if any(type(x) is not int or x <= 0 for x in asdict(self).values()):
            raise ValueError('POSITIVE_INTEGER_DELIVERY_CONFIGURATION')
        if self.kalshi_reads_per_second > 20 or self.polymarket_reads_per_second > 30:
            raise ValueError('CONFIGURED_EXTERNAL_READ_CEILING')


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def growth_score(daily_wealth, opportunity_pnl, evidence_strength=10., consistency=1.5):
    """Daily rows are complete comparable wealth (cash+inventory-debt-fees).

    opportunity_pnl is an iterable of (independence_cluster, net PnL). Aggregate
    all receipts/costs in each cluster before counting it. Caller must provide
    economic event grouping; anonymous individual fills are not opportunities.
    """
    if len(daily_wealth) < 2 or not math.isfinite(evidence_strength) or evidence_strength <= 0 or not math.isfinite(consistency) or consistency < 0:
        raise ValueError('TWO_COMPLETE_DAYS_AND_VALID_SCORE_PARAMETERS_REQUIRED')
    returns = []
    for start, end in daily_wealth:
        if any(type(x) not in (int, float) or not math.isfinite(x) for x in (start, end)) or start <= 0:
            raise ValueError('COMPLETE_FINITE_WEALTH_REQUIRED')
        if end <= 0:
            return {'reward': None, 'bankrupt': True, 'eligible': False}
        returns.append(math.log(end/start))
    clusters = {}
    for key, pnl in opportunity_pnl:
        if not isinstance(key, str) or not key or type(pnl) not in (int, float) or not math.isfinite(pnl):
            raise ValueError('EXPLICIT_INDEPENDENT_EVENT_AND_NET_PNL_REQUIRED')
        clusters[key] = clusters.get(key, 0) + pnl
    n = sum(pnl > 0 for pnl in clusters.values())
    mean = statistics.mean(returns); sigma = statistics.stdev(returns)
    g = mean-consistency*sigma/math.sqrt(len(returns))
    return {'reward': g*math.sqrt(n/(n+evidence_strength)) if g > 0 else g,
            'G': g, 'daily_log_returns': returns, 'sample_daily_std': sigma,
            'D': len(returns), 'N': n, 'bankrupt': False, 'eligible': True}
