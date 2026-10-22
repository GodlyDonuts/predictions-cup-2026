"""Exact existing-worker draft-yield projection, never a cash credit."""
from decimal import Decimal


def project(view, exchange):
    base = view['admissionProjectionAfterOrdinaryDraftRetirement']
    target = view['managedTargetAdmissionProjections'].get(exchange)
    selected = target or base
    ids = sorted(set(base['eligibleQuoteIds'] + (target['targetQuoteIds'] if target else [])))
    assert len(ids) == len(base['eligibleQuoteIds']) + (len(target['targetQuoteIds']) if target else 0)
    assert selected['actualCashCredited'] == '0.000000' and selected['executionAuthority'] is False
    assert selected['alreadyRetired'] is False
    active = {q['id']: q for q in view['activeNativeOrders']}
    assert len(active) == len(view['activeNativeOrders'])
    assert not set(ids).intersection(view['knownUnclosedAcknowledgments'])
    assert Decimal(view['policy']['feePerShare']) == 0 and view['policy']['feeBps'] == 0
    released = Decimal(0)
    for qid in ids:
        q = active[qid]
        assert q['retired'] is False and q['filled'] == 0 and q['action'] in ('buy', 'sell')
        if q['action'] == 'buy':
            released += Decimal(q['price']) * q['quantity']
    current = view['account']['reservedCash']
    expected = selected['expectedReservedCash']
    assert Decimal(current) - released == Decimal(expected) >= 0
    return dict(capturedReservedCash=current, expectedReservedCash=expected,
                quoteIds=ids, actualCashCredited='0.000000', executionAuthority=False)
