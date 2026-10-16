"""Narrow flat/single-market sizing counterfactual, not an Engine replay or EV model."""
import hashlib
import json
import subprocess
from pathlib import Path

SCALE = 1_000_000


def fee(price, bps=0, per_share=0):
    assert 0 <= bps and 0 <= per_share
    return (price * bps + 9999) // 10000 + per_share


def compare(bid, ask, bid_qty, ask_qty, *, quantity=10000, cash=90000*SCALE,
            total_room=90000*SCALE, market_room=10000*SCALE, edge=1000,
            depth_bps=10000, fee_bps=0, fee_share=0):
    assert 0 < bid < ask < SCALE and bid % 5000 == ask % 5000 == 0
    assert min(bid_qty, ask_qty, quantity) > 0 and 0 < depth_bps <= 10000
    assert min(cash, total_room, market_room) >= 0 and edge > 0
    micro = (ask*bid_qty + bid*ask_qty)//(bid_qty+ask_qty)
    prices = {'NO': SCALE-ask, 'YES': bid}
    depths = {'NO': ask_qty, 'YES': bid_qty}
    values = {'NO': SCALE-micro, 'YES': micro}
    units = {s: p+fee(p, fee_bps, fee_share) for s,p in prices.items()}
    passes = {s: values[s]-prices[s]-fee(prices[s],fee_bps,fee_share)>=edge for s in prices}
    pair_ok = ask-bid-fee(bid,fee_bps,fee_share)-fee(SCALE-ask,fee_bps,fee_share)>=2*edge

    def size(paired):
        eligible = list(prices) if paired and pair_ok else ([] if paired else [s for s in prices if passes[s]])
        out={}; available=cash; headroom=total_room; room=market_room
        for side in eligible:  # native rank ties then label: NO before YES
            n=min(quantity, depths[side]*depth_bps//10000)
            mate=next(iter(out),None)
            opposite=next((s for s in eligible if s!=side),None) if paired and mate is None else None
            if mate is not None:n=min(n,out[mate]['quantity'])
            if opposite is not None:n=min(n,depths[opposite]*depth_bps//10000)
            unit=units[side]+(units[opposite] if opposite is not None else 0)
            n=min(n,min(available,headroom,room)//unit)
            if n:
                cost=n*units[side];available-=cost;headroom-=cost;room-=cost
                out[side]={'quantity':n,'limitMicros':prices[side],'reservedMicros':cost}
        return {'drafts':out,'reservedMicros':sum(q['reservedMicros'] for q in out.values()),
                'physicalAfterSameBookSideFilter':{s:q for s,q in out.items() if passes[s]}}
    return {'micropriceMicros':micro,'sideEdgeMicros':{s:values[s]-prices[s]-fee(prices[s],fee_bps,fee_share) for s in prices},
            'sidePasses':passes,'pairSpreadPasses':pair_ok,'pairedSpread':size(True),'microprice':size(False)}


def main():
    repo=Path(__file__).resolve().parents[2]
    paths=['moloch/native/src/engine.rs','moloch/native/src/lib.rs','moloch/live-runtime/src/ordinary_side_edge.rs']
    report={'sourceBaseCommit':'bfd1e368','sourceFilesGitBlobSha256':{p:hashlib.sha256(subprocess.check_output(['git','show','bfd1e368:'+p],cwd=repo)).hexdigest() for p in paths},
      'scope':'Flat one exchange, no prior drafts/reservations, same fixed current book, zero fees, inside/backoff0, full synthetic funding room. Exact planner arithmetic excerpt, not full Engine replay.',
      'actualSnapshotOrigin':'Finite numerical snapshots supplied by execution owner; no new account reads or raw-receipt membership verification.',
      'counterfactualFunding':{'cashMicros':90000*SCALE,'totalRoomMicros':90000*SCALE,'marketRoomMicros':10000*SCALE,'quantityCap':10000},
      'examples':{'qid8086':compare(40000,45000,33171,487),'qid8126':compare(695000,700000,4451,38)},
      'executionAuthority':False,'profitOrFillPrediction':False}
    print(json.dumps(report,indent=2,sort_keys=True))


if __name__=='__main__':main()
