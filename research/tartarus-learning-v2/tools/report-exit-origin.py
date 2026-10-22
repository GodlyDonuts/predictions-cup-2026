"""Classify an existing receipt/FIFO audit using immutable native quote metadata.

Reads only existing files. Later quote metadata does not extend the audit's
financial interval or establish current account value.
"""
import argparse
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
from pathlib import Path


def run(a):
    audit_raw, bank_raw = a.audit.read_bytes(), a.bank_report.read_bytes()
    audit, bank = json.loads(audit_raw), json.loads(bank_raw)
    assert audit['allNewFillsMatchOriginalOfficialReceipts'] and audit['remainingFifoExactlyMatches']
    quotes = {str(q['id']): q for q in bank['orders']}
    assert len(quotes) == len(bank['orders'])
    groups, rows = {}, []
    for qid, order in audit['orders'].items():
        q = quotes[qid]
        assert all(q[k] == order[k] for k in ('exchangeId', 'side', 'action'))
        if isinstance(q.get('reviewedRankCampaign'), dict):
            kind = 'reviewed_monetary'
        elif isinstance(q.get('reviewedInventoryExit'), dict):
            kind = 'reviewed_inventory_exit'
        elif q.get('inventoryExitReason') == 'maximumAge':
            kind = 'automatic_maximum_age'
        elif isinstance(q.get('reviewedFundamentalEntry'), dict):
            kind = 'reviewed_fundamental_entry'
        else:
            kind = 'ordinary'
        group = groups.setdefault(kind, dict(filledOrders=0, realizedNet=Decimal(0),
                                            buys=0, sells=0))
        group['filledOrders'] += 1
        group['realizedNet'] += Decimal(order['realizedNet'])
        group['buys' if order['action'] == 'buy' else 'sells'] += 1
        rows.append(dict(qid=qid, origin=kind, **{k: order[k] for k in
            ('actualOrderId', 'exchangeId', 'side', 'action', 'shares', 'firstExecutedAt', 'realizedNet')}))
    assert sum((x['realizedNet'] for x in groups.values()), Decimal(0)) == Decimal(audit['incrementalRealizedNet'])
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(),
        auditObservedUtc=audit['observedUtc'], auditSha256=hashlib.sha256(audit_raw).hexdigest(),
        auditOriginalReportSha256=audit['reportSha256'],
        quoteMetadataReportSha256=hashlib.sha256(bank_raw).hexdigest(),
        quoteMetadataReportAt=bank['report']['at'], sameFinancialIntervalAsAudit=True,
        accountRequests=0, currentAccountValueClaimed=False, groups=groups, orders=rows)
    with a.output.open('x', encoding='utf-8') as stream:
        json.dump(result, stream, default=str, indent=2)
        stream.write('\n')
    print(json.dumps(groups, default=str))


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('audit', 'bank-report', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    run(p.parse_args())
