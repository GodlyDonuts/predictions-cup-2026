"""Lossless, field-aware compression of a declared rank scenario matrix.

Rows merge only when every contract payout, exception class, and every supplied
opposing-field threshold agree. Each probability model keeps its own weights.
This changes neither economic assumptions nor source clocks and grants no live
authority. All field models must be supplied before compression.
"""
from copy import deepcopy
from decimal import Decimal
import hashlib
import json
import re

PPM = 1_000_000
PAYOUTS = {"resolved_yes", "resolved_no", "refund_principal"}
MONEY = re.compile(r"-?\d+(?:\.\d{1,6})?\Z")


def _need(ok, reason):
    if not ok:
        raise ValueError(reason)


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode()


def _sha(value):
    return hashlib.sha256(_canonical(value)).hexdigest()


def _text(value):
    return type(value) is str and 0 < len(value) <= 4096


def coalesce(scenarios, probability_models, field_models, maximum_output=128):
    """Preserve every modeled wealth/threshold score, including ties and tails.

    The caller remains responsible for complete contract/source semantics and
    supplying ALL state-dependent field models. A later field model requires
    recompression from the original rows, never invention of a latent mapping.
    """
    _need(type(maximum_output) is int and 1 <= maximum_output <= 256,
          "NATIVE_OUTPUT_BOUND")
    _need(type(scenarios) is list and 1 <= len(scenarios) <= 4096,
          "RESEARCH_INPUT_BOUND")
    _need(type(probability_models) is list and 1 <= len(probability_models) <= 8,
          "PROBABILITY_MODEL_BOUND")
    _need(type(field_models) is list and 1 <= len(field_models) <= 8,
          "COMPLETE_FIELD_MODEL_SET_REQUIRED")
    n = len(scenarios)
    contracts = None
    ids = set()
    for row in scenarios:
        _need(type(row) is dict and set(row) ==
              {"id", "kind", "assumptionLabel", "payouts"},
              "UNMODELED_SCENARIO_FIELDS")
        _need(_text(row["id"]) and row["id"] not in ids and
              _text(row["kind"]) and _text(row["assumptionLabel"]),
              "SCENARIO_ID_OR_LABEL")
        ids.add(row["id"])
        payouts = row["payouts"]
        _need(type(payouts) is dict and 1 <= len(payouts) <= 64 and
              all(_text(k) and type(v) is str and v in PAYOUTS for k, v in payouts.items()),
              "COMPLETE_PAYOUT_VECTOR_REQUIRED")
        if contracts is None:
            contracts = set(payouts)
        _need(set(payouts) == contracts, "INCOMPLETE_PAYOUT_VECTOR")
    for models, weights, allowed, code in [
        (probability_models, "weightsPpm", {"id", "assumptionLabel", "weightsPpm"},
         "PROBABILITY_MODEL"),
        (field_models, "thresholds", {"id", "assumptionLabel", "thresholds", "completeField"},
         "FIELD_MODEL"),
    ]:
        seen = set()
        for model in models:
            _need(type(model) is dict and set(model) == allowed and
                  _text(model["id"]) and model["id"] not in seen and
                  _text(model["assumptionLabel"]), code)
            seen.add(model["id"])
            vector = model[weights]
            _need(type(vector) is list and len(vector) == n, code+"_DIMENSION")
            if weights == "weightsPpm":
                _need(all(type(w) is int and 0 <= w <= PPM for w in vector)
                      and sum(vector) == PPM, "PROBABILITY_MASS")
            else:
                _need(model["completeField"] is False and
                      all(type(v) is str and len(v) <= 48 and MONEY.fullmatch(v) and
                          Decimal(v).is_finite() for v in vector),
                      "FIELD_THRESHOLD_OR_FALSE_COMPLETENESS")
    groups = []
    signatures = {}
    for index, row in enumerate(scenarios):
        signature = _canonical([row["kind"], row["payouts"],
                                [m["thresholds"][index] for m in field_models]])
        if signature not in signatures:
            signatures[signature] = len(groups)
            groups.append([])
        groups[signatures[signature]].append(index)
    _need(len(groups) <= maximum_output, "FIELD_AWARE_MATRIX_EXCEEDS_NATIVE_BOUND")
    merged = []
    provenance = []
    for group in groups:
        original = scenarios[group[0]]
        ident = "coalesced-"+_sha([original["kind"], original["payouts"],
                                  [m["thresholds"][group[0]] for m in field_models]])
        merged.append({"id": ident, "kind": original["kind"],
                       "assumptionLabel": "Lossless payout and opposing-field equivalence; original labels/rows retained in coalescing evidence; no new probability assumption",
                       "payouts": deepcopy(original["payouts"])})
        provenance.append({"id": ident, "originalIndices": group,
                           "originalRows": [deepcopy(scenarios[i]) for i in group]})
    probabilities = deepcopy(probability_models)
    for old, new in zip(probability_models, probabilities):
        new["weightsPpm"] = [sum(old["weightsPpm"][i] for i in g) for g in groups]
        _need(sum(new["weightsPpm"]) == PPM, "MASS_CHANGED")
    fields = deepcopy(field_models)
    for old, new in zip(field_models, fields):
        new["thresholds"] = [old["thresholds"][g[0]] for g in groups]
    result = {"schemaVersion": 1, "scenarios": merged,
              "probabilityModels": probabilities, "fieldModels": fields,
              "executionAuthority": False, "probabilityCalibrationAdded": False}
    result["coalescingEvidence"] = {
        "inputSha256": _sha([scenarios, probability_models, field_models]),
        "outputMatrixSha256": _sha([merged, probabilities, fields]),
        "inputScenarioCount": n, "outputScenarioCount": len(groups),
        "allFieldModelIds": [m["id"] for m in field_models],
        "eachSourceModelRetainedSeparately": True, "groups": provenance,
        "laterFieldModelRequiresOriginalRows": True,
    }
    return result
