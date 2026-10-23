"""Audit original official marks against the existing native view; no requests.

Only aggregate economics and hashes are exported. Account personal fields stay
in their original private receipts. Marks are not executable liquidation.
"""
from pathlib import Path
from decimal import Decimal as D
from datetime import datetime, timezone
import argparse
import hashlib
import json


def run(a):
    view_path=a.runtime/'tartarus-current-native-state.json'; view_raw=view_path.read_bytes()
    view=json.loads(view_raw); refs=view['originalObservationReceiptClocks']
    account_ref=next(r for r in refs if r['endpoint'].startswith('/account?'))
    positions_ref=next(r for r in refs if '/portfolio/positions' in r['endpoint'])
    def read(ref):
        raw=(a.runtime/'receipts'/(ref['receiptSha256']+'.json')).read_bytes()
        parsed=json.loads(raw,parse_float=D)
        assert parsed['status']==200 and parsed['endpoint']==ref['endpoint']
        assert parsed['receivedAt']==ref['receivedAt'] and parsed['requestStartedAt']==ref['requestStartedAt']
        return raw,parsed
    account_raw,account=read(account_ref); positions_raw,positions=read(positions_ref)
    cash=D(str(account['data']['balance'])); assert cash==D(view['account']['cash'])
    rows=positions['data']['positions']; official={}
    for row in rows:
        n=D(str(row['quantity'])); assert n==int(n)
        if n:
            ex=str(row['exchangeId']); assert ex not in official
            official[ex]=int(n)
    native={ex:sum(lot['quantity'] for lot in sides.get('YES',[]))-sum(lot['quantity'] for lot in sides.get('NO',[]))
            for ex,sides in view['account']['positions'].items()}
    native={ex:n for ex,n in native.items() if n}; assert official==native, 'OFFICIAL_QUANTITIES_DIFFER_FROM_NATIVE'
    marks=sum((D(str(row['marketValue'])) for row in rows),D(0))
    basis=sum((D(str(row['costBasis'])) for row in rows),D(0))
    unrealized=sum((D(str(row['unrealizedPnl'])) for row in rows),D(0))
    summary=positions['data']['summary']
    summary_marks=D(str(summary['totalMarketValue'])); summary_basis=D(str(summary['totalCostBasis']))
    summary_unrealized=D(str(summary['totalUnrealizedPnl']))
    baseline_raw=a.initial_observation.read_bytes(); baseline=json.loads(baseline_raw)
    initial=D(baseline['officialInitialBalance']); assert initial>0
    assert baseline['tournamentId'] in account_ref['endpoint'], 'INITIAL_TOURNAMENT_MISMATCH'
    assert view_path.read_bytes()==view_raw, 'NATIVE_VIEW_CHANGED_DURING_AUDIT'
    result=dict(observedUtc=datetime.now(timezone.utc).isoformat(),accountReceiptAt=account['receivedAt'],
        positionsReceiptAt=positions['receivedAt'],tournamentId=baseline['tournamentId'],officialInitialBalance=str(initial),
        officialCash=str(cash),officialPositionMarks=str(marks),cashPlusOfficialMarks=str(cash+marks),
        cashPlusMarksMinusInitial=str(cash+marks-initial),markedRoiPercentVersusOfficialInitial=str((cash+marks-initial)/initial*100),
        officialReportedBasis=str(basis),officialReportedUnrealized=str(unrealized),
        reportedUnrealizedMinusMarksLessBasis=str(unrealized-(marks-basis)),
        officialSummaryMarketValue=str(summary_marks),officialSummaryBasis=str(summary_basis),
        officialSummaryUnrealized=str(summary_unrealized),rowMarksMinusOfficialSummary=str(marks-summary_marks),
        rowBasisMinusOfficialSummary=str(basis-summary_basis),rowUnrealizedMinusOfficialSummary=str(unrealized-summary_unrealized),
        allOfficialQuantitiesMatchNative=True,allOfficialCashMatchesNative=True,matchedPositionCount=len(native),
        nativeViewWrittenAt=view['writtenAt'],nativeFillRows=view['account']['fillCount'],
        sourceRawSha256={k:hashlib.sha256(raw).hexdigest() for k,raw in [('account',account_raw),('positions',positions_raw),('nativeView',view_raw),('initialObservation',baseline_raw)]},
        rawAccountPersonalFieldsPublished=False,marksAreExecutableLiquidation=False,dailyPnlEstablished=False,
        simultaneousSnapshotClaimed=False,extraAccountRequests=0)
    with a.output.open('x') as f: json.dump(result,f,indent=2); f.write('\n')
    print(json.dumps(result))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['runtime','initial-observation','output']: p.add_argument('--'+n,type=Path,required=True)
    run(p.parse_args())
