"""Project a shared research matrix without regenerating joint assumptions.

Every supplied held contract is mandatory. The caller must supply its complete
holding scope; this pure research function cannot authenticate a live account.
All field models survive projection before lossless field-aware compression.
"""
from copy import deepcopy
import hashlib
import json

from .scenario_coalescing import coalesce


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
        ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def project(scenarios, probability_models, field_models, *, retained_contract_ids,
            complete_held_contract_ids, maximum_native_scenarios=128):
    """Keep the exact joint distribution relevant to a candidate/account subset.

    No omitted candidate is included in a plan. Existing account compilation and
    native preflight must independently establish complete holdings/obligations.
    Source freshness and original provenance are not changed by this operation.
    """
    need(type(scenarios) is list and 1 <= len(scenarios) <= 4096, "PROJECTION_ROWS")
    need(type(retained_contract_ids) is list and 1 <= len(retained_contract_ids) <= 64
         and all(type(c) is str and c for c in retained_contract_ids)
         and len(set(retained_contract_ids)) == len(retained_contract_ids), "PROJECTION_SCOPE")
    need(type(complete_held_contract_ids) is list
         and all(type(c) is str and c for c in complete_held_contract_ids)
         and len(set(complete_held_contract_ids)) == len(complete_held_contract_ids), "HELD_SCOPE_REQUIRED")
    retained = set(retained_contract_ids)
    need(set(complete_held_contract_ids) <= retained, "PROJECTION_OMITS_HOLDING")
    scope = None
    projected = []
    for row in scenarios:
        need(type(row) is dict and set(row) == {"id", "kind", "assumptionLabel", "payouts"},
             "UNMODELED_SCENARIO_FIELDS")
        payouts = row["payouts"]
        need(type(payouts) is dict and 1 <= len(payouts) <= 512
             and all(type(c) is str and c and p in {"resolved_yes", "resolved_no", "refund_principal"}
                     for c, p in payouts.items()), "PROJECTION_COMPLETE_MATRIX")
        if scope is None:
            scope = set(payouts)
        need(set(payouts) == scope and retained <= scope, "PROJECTION_INCOMPLETE_SCOPE")
        projected.append(dict(row, payouts={c:deepcopy(payouts[c]) for c in sorted(retained)}))
    reduced = coalesce(projected, probability_models, field_models,
                       maximum_output=maximum_native_scenarios)
    reduced["projection"] = {
        "originalScenariosSha256":digest(scenarios),
        "originalProbabilityModelsSha256":digest(probability_models),
        "originalFieldModelsSha256":digest(field_models),
        "originalContractCount":len(scope), "retainedContractIds":sorted(retained),
        "callerSuppliedCompleteHeldContractIds":sorted(complete_held_contract_ids),
        "omittedCandidateContractIds":sorted(scope-retained),
        "independentAccountAndLiabilityVerificationRequired":True,
        "sourceClocksRenewed":False, "executionAuthority":False,
    }
    return reduced
