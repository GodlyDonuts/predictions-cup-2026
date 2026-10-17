"""Bounded, declared whole-account scenarios; never execution authority.

Contract A in research/tartarus-20261017/CONTRACTS.md. All *_micros amounts
are exact integer millionths. A source digest pins declared evidence, not its
independent authenticity, exhaustive resolution semantics, or calibrated odds.
"""
from copy import deepcopy
import hashlib
import json
import re

UNIT = 1_000_000
MAX_INTEGER = 2**63 - 1
MAX_CONTRACTS = 512
MAX_SCENARIOS = 4096
MAX_CELLS = 262_144
MAX_HOLDINGS = 1024
MAX_LOTS = 16_384
MAX_PENDING = 1024
PAYOUTS = frozenset(("resolved_yes", "resolved_no", "refund_principal"))
_SHA = re.compile(r"[0-9a-f]{64}\Z")
_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}\Z")


class AccountScenarioError(ValueError):
    """A rejected/unavailable research input; code is stable and nonfinancial."""

    def __init__(self, code):
        self.code = code
        super().__init__(code)


def _need(ok, code):
    if not ok:
        raise AccountScenarioError(code)


def _integer(value, code, minimum=0):
    _need(type(value) is int and minimum <= value <= MAX_INTEGER, code)
    return value


def _amount(value, code):
    _need(type(value) is int and -MAX_INTEGER <= value <= MAX_INTEGER, code)
    return value


def _text(value, code, maximum=8192):
    _need(type(value) is str and 0 < len(value) <= maximum and value.strip() != "", code)
    return value


def _identity(value, code):
    _need(type(value) is str and _ID.fullmatch(value) is not None, code)
    return value


def _sha(value, code):
    _need(type(value) is str and _SHA.fullmatch(value) is not None, code)
    return value


def _shape(value, fields, code):
    _need(type(value) is dict and len(value) == len(fields) and set(value) == set(fields), code)


def _list(value, bound, code):
    _need(type(value) is list and len(value) <= bound, code)
    return value


COMMON = {"schema_version", "evidence_class", "as_of_ms", "valid_until_ms",
          "source_sha256", "unmodeled_state_statement"}
HOLDING_FIELDS = {"contract_id", "exchange_id", "side", "quantity", "lots", "source_sha256"}


def _evidence(value, decision_ms, name):
    _need(type(value["schema_version"]) is int and value["schema_version"] == 1, name + "_SCHEMA_VERSION")
    _text(value["evidence_class"], name + "_EVIDENCE_CLASS", 128)
    _text(value["unmodeled_state_statement"], name + "_UNMODELED_STATEMENT")
    _sha(value["source_sha256"], name + "_SOURCE_SHA256")
    start = _integer(value["as_of_ms"], name + "_AS_OF")
    end = _integer(value["valid_until_ms"], name + "_VALID_UNTIL")
    _need(start < end, name + "_SOURCE_WINDOW")
    _need(start <= decision_ms, name + "_SOURCE_FUTURE")
    _need(decision_ms < end, name + "_SOURCE_EXPIRED")


def _holding(value):
    _shape(value, HOLDING_FIELDS, "HOLDING_SHAPE")
    _identity(value["contract_id"], "HOLDING_CONTRACT_ID")
    _identity(value["exchange_id"], "HOLDING_EXCHANGE_ID")
    _need(type(value["side"]) is str and value["side"] in ("YES", "NO"), "HOLDING_SIDE")
    _sha(value["source_sha256"], "HOLDING_SOURCE_SHA256")
    quantity = _integer(value["quantity"], "HOLDING_QUANTITY")
    lots = _list(value["lots"], MAX_LOTS, "HOLDING_LOT_BOUND")
    total = basis = 0
    for lot in lots:
        _shape(lot, {"quantity", "unit_cost_micros"}, "LOT_SHAPE")
        q = _integer(lot["quantity"], "LOT_QUANTITY", 1)
        cost = _integer(lot["unit_cost_micros"], "LOT_UNIT_COST")
        total = _integer(total + q, "HOLDING_QUANTITY_RANGE")
        basis = _integer(basis + q * cost, "HOLDING_BASIS_RANGE")
    _need(total == quantity, "HOLDING_FIFO_QUANTITY_MISMATCH")
    return basis


def fifo_sell(holding, quantity):
    """Pure hypothetical FIFO removal, with remaining original principal.

    No proceeds, fees, fills, account cash, or approval are inferred. Lots retain
    their original order and costs. Zero sale is explicit; excess sale rejects.
    """
    original_basis = _holding(holding)
    quantity = _integer(quantity, "FIFO_SELL_QUANTITY")
    _need(quantity <= holding["quantity"], "FIFO_SELL_EXCEEDS_HOLDING")
    left = quantity
    removed = 0
    remaining = []
    for lot in holding["lots"]:
        take = min(left, lot["quantity"])
        removed += take * lot["unit_cost_micros"]
        left -= take
        if take < lot["quantity"]:
            remaining.append({"quantity": lot["quantity"] - take,
                              "unit_cost_micros": lot["unit_cost_micros"]})
    _need(left == 0, "FIFO_SELL_INCOMPLETE")
    return {"remaining_lots": remaining, "removed_basis_micros": removed,
            "remaining_quantity": holding["quantity"] - quantity,
            "remaining_basis_micros": original_basis - removed,
            "execution_authorized": False, "calibrated_probability_claimed": False}


def compile_account(account, scenarios, decision_ms):
    """Compile all explicit holdings against a bounded declared joint matrix.

    Wealth = cash (already including reserved cash) - terminal debt + payouts.
    Available acquisition cash alone subtracts reservations. No pending liability
    may be guessed to be zero: v1 rejects every nonempty obligation list.
    Each scenario covers the entire registry exactly, including flat contracts
    which downstream candidate searches may buy. No independence is assumed.
    """
    decision_ms = _integer(decision_ms, "DECISION_CLOCK")
    _shape(account, COMMON | {"cash_micros", "reserved_cash_micros", "terminal_debt_micros",
                              "holdings", "pending_obligations"}, "ACCOUNT_SHAPE")
    _shape(scenarios, COMMON | {"contracts", "scenarios"}, "SCENARIO_SET_SHAPE")
    _evidence(account, decision_ms, "ACCOUNT")
    _evidence(scenarios, decision_ms, "SCENARIOS")
    cash = _integer(account["cash_micros"], "CASH_AMOUNT")
    reserved = _integer(account["reserved_cash_micros"], "RESERVED_CASH_AMOUNT")
    debt = _integer(account["terminal_debt_micros"], "TERMINAL_DEBT_AMOUNT")
    _need(reserved <= cash, "RESERVATIONS_EXCEED_CASH")
    obligations = _list(account["pending_obligations"], MAX_PENDING, "PENDING_OBLIGATION_SHAPE_OR_BOUND")
    _need(not obligations, "PENDING_OBLIGATIONS_UNAVAILABLE")
    holdings = _list(account["holdings"], MAX_HOLDINGS, "HOLDING_BOUND")
    contracts = _list(scenarios["contracts"], MAX_CONTRACTS, "CONTRACT_BOUND")
    rows = _list(scenarios["scenarios"], MAX_SCENARIOS, "SCENARIO_BOUND")
    _need(len(contracts) > 0 and len(rows) > 0, "EMPTY_SCENARIO_MATRIX")
    _need(len(contracts) * len(rows) <= MAX_CELLS, "SCENARIO_CELL_BOUND")
    registry = {}
    exchanges = set()
    for c in contracts:
        _shape(c, {"contract_id", "exchange_id", "semantic_source_sha256"}, "CONTRACT_SHAPE")
        cid = _identity(c["contract_id"], "CONTRACT_ID")
        ex = _identity(c["exchange_id"], "CONTRACT_EXCHANGE_ID")
        _sha(c["semantic_source_sha256"], "CONTRACT_SEMANTIC_SOURCE_SHA256")
        _need(cid not in registry and ex not in exchanges, "CONTRACT_IDENTITY_CONFLICT")
        registry[cid] = ex
        exchanges.add(ex)
    seen = set()
    basis = {}
    total_lots = 0
    for h in holdings:
        b = _holding(h)
        cid, ex, side = h["contract_id"], h["exchange_id"], h["side"]
        _need(registry.get(cid) == ex, "HELD_CONTRACT_NOT_IN_EXACT_REGISTRY")
        _need((cid, side) not in seen, "DUPLICATE_OR_CONFLICTING_HOLDING")
        seen.add((cid, side))
        basis[(cid, side)] = b
        total_lots += len(h["lots"])
        _need(total_lots <= MAX_LOTS, "ACCOUNT_TOTAL_LOT_BOUND")
    ids = []
    kinds = []
    labels = []
    payout_rows = []
    wealth = []
    id_set = set()
    for scenario in rows:
        _shape(scenario, {"id", "kind", "payouts", "assumption_label"}, "SCENARIO_SHAPE")
        sid = _identity(scenario["id"], "SCENARIO_ID")
        _need(sid not in id_set, "DUPLICATE_SCENARIO_ID")
        id_set.add(sid)
        _need(type(scenario["kind"]) is str and scenario["kind"] in ("ordinary", "exceptional"), "SCENARIO_KIND")
        _text(scenario["assumption_label"], "SCENARIO_ASSUMPTION", 4096)
        payouts = scenario["payouts"]
        _need(type(payouts) is dict and len(payouts) == len(registry) and set(payouts) == set(registry), "SCENARIO_REGISTRY_COVERAGE")
        _need(all(type(p) is str and p in PAYOUTS for p in payouts.values()), "UNKNOWN_SCENARIO_PAYOUT")
        value = _amount(cash - debt, "TERMINAL_WEALTH_RANGE")
        for h in holdings:
            payout = payouts[h["contract_id"]]
            if payout == "refund_principal":
                paid = basis[(h["contract_id"], h["side"])]
            else:
                yes = payout == "resolved_yes"
                paid = h["quantity"] * UNIT if yes == (h["side"] == "YES") else 0
            value = _amount(value + paid, "TERMINAL_WEALTH_RANGE")
        ids.append(sid)
        kinds.append(scenario["kind"])
        labels.append(scenario["assumption_label"])
        payout_rows.append(deepcopy(payouts))
        wealth.append(value)
    # This derived canonical digest includes the full declarations, not merely
    # claimed source hashes. It still does not authenticate underlying sources.
    canonical = json.dumps({"account": account, "scenarios": scenarios, "decision_ms": decision_ms},
                           sort_keys=True, separators=(",", ":"), ensure_ascii=True, allow_nan=False).encode()
    return {"schema_version": 1, "scenario_ids": ids, "baseline_wealth_micros": wealth,
            "available_cash_micros": cash - reserved, "cash_micros": cash,
            "reserved_cash_micros": reserved, "terminal_debt_micros": debt,
            "holdings": deepcopy(holdings), "contracts": deepcopy(contracts),
            "scenario_payouts": payout_rows, "scenario_kinds": kinds, "scenario_assumptions": labels,
            "as_of_ms": max(account["as_of_ms"], scenarios["as_of_ms"]),
            "valid_until_ms": min(account["valid_until_ms"], scenarios["valid_until_ms"]),
            "source_sha256": hashlib.sha256(canonical).hexdigest(),
            "evidence_class": account["evidence_class"] if account["evidence_class"] == scenarios["evidence_class"] else "mixed",
            "unmodeled_state_statement": "Declared sampled matrix, not exhaustive. Account: "
                + account["unmodeled_state_statement"] + " Scenarios: " + scenarios["unmodeled_state_statement"],
            "scenario_matrix_exhaustive": False, "decision_ms": decision_ms,
            "provenance": {"account_source_sha256": account["source_sha256"],
                           "scenario_source_sha256": scenarios["source_sha256"],
                           "account_evidence_class": account["evidence_class"],
                           "scenario_evidence_class": scenarios["evidence_class"],
                           "source_body_membership_independently_verified": False},
            "execution_authorized": False, "calibrated_probability_claimed": False}
