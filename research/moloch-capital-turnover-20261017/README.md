# Frozen capital-turnover audit — October17

**No early liquidation is justified by this packet alone.** Two historical depth curves could have produced positive FIFO accounting gains, but both sell below their installed reviewed payout scenarios. Realizing a gain does not itself improve expected wealth. No sale, current executable quote, profit forecast, authority or policy change is claimed.

[aggregate.json](aggregate.json) derives from the private R26 incident packet, not the subsequent R27 runtime. Its16 original file pins,5,531 native suffix rows483808–489338 and complete21,748-row dispatch chain were independently checked using original raw bytes, removing only the unique outer `,"hash":"…"` fragment. This preserves numeric spelling. It is not another full Rust financial replay or independent authentication of underlying HTTP/WS bodies. Raw source/account data remain private. Strategy source reference: `d4d1e8aa82d68723b63a0757247b64e0570fce6b`.

At native489338/12:53:28.548UTC: cash3,613.785, owned cost91,851.730,382 actual fill rows; attributed realized−114.400 consists of new-trading−184.970 and inherited+70.570. None of the hypothetical gains below is added to that ledger.

| Historical sale slice | FIFO basis | Visible-depth proceeds / gain | Fixed limit / gain if all filled at that limit | Reviewed scenario value / value forgone at visible depth |
|---|---:|---:|---:|---:|
| MA1068YES, all7,061 |6,624.170|6,716.110 / +91.940|.950 / +83.780|6,920.4861 /204.3761|
| AK1067YES, first10,000 FIFO shares |3,113.770|3,194.170 / +80.400|.315 / +36.230|4,917.900 /1,723.730|

All use the original zero-fee policy and actual recorded depth. A limit is a price constraint **if** filled, not a fill promise; later changes, partial execution and liability/freshness checks remain. MA's1632 shares at.955 plus5429 at.950 support the first curve. AK's8834 at.320 plus1166 at.315 support the second. They are not top-price marks applied to an entire position.

**Clock correction:** MA source12:52:59.482157500UTC, receipt **12:53:00.374UTC**, processing12:53:01.200UTC. The earlier chat's13:00.374 was a typo. AK source12:53:24.961973200UTC, receipt12:53:25.221UTC, processing12:53:25.316UTC. Exact original row/version/hash pins are in the aggregate. These books are historical and cannot authorize a present sale.

## Why the existing exits hold

`fundamental_policy.rs::{existing_exit_hint,prepare_existing_exit,assess_quantity}` requires a current reviewed value exit, genuine deadline or explicit reviewed revocation. MA's owner assumption is.990 with.010 unestimated exceptional allowance/zero exceptional payout: scenario value.9801/share. Its original V1 objective remains an **October19,17:00UTC holding deadline**, not an explicit forecast of that day's liquidation price. The primary CTM Markey point was censored `>99.9%`; it is not an exact.999 estimate or confidence bound. The printed General ballot has additional candidates/write-ins. The scenario is an owner assumption, not calibrated cash at the deadline.

AK uses the original CTM incumbent win point.507 and.030 exceptional allowance: scenario value.49179/share, holding for actual settlement scheduled November4,17:00UTC, including delay. RCV transfers, the second Republican's classification, party/resolver/replacement/refund risk and model disagreement remain. The retained44–46% alternative points still exceed the approximately32.93% conditional break-even at this old sale curve; they are not averaged or calibrated bounds. Both reviews' original source clocks and expiries remain unchanged (MA22:51:23.996 and AK19:02:56.239UTC on October17).

At the old curves, ignoring other constraints, MA would need a conditional win point around.960763 rather than.990, or a source-supported alternative yielding over204.3761 net on released6,716.110 (3.0431% total) to offset the scenario value forgone. AK's corresponding point is.329296 rather than.507; its alternative would need over1,723.730 net on3,194.170 (53.9649% total). Those are arithmetic sensitivity thresholds, **not** instructions to change probabilities, exceptions or holding times. Fixed-limit hurdles are higher:3.1684%/56.1238%. Do not assume repeated reinvestment/fills or annualize an unknown cash-release horizon.

A fixed-final-basis screen of2,726 non-invalid retained owned-side books found **zero** bids reaching the original worst-lot basis plus.005 native gain. It does not prove complete historical opportunity coverage or that every remaining lot existed throughout the suffix. No profitable inherited exit was established by this screen. Managed FIFO accounting gains above do not override their different reviewed-value objectives.

## Cash and the smallest justified next decision

The original unused851NO draft15469 is680 shares at.875:595 principal. Existing cash already covers it without a sale. Its reviewed.8892 scenario implies9.656 total surplus if fully acquired at that limit, far below either held-value sacrifice. The packet's3,613.120 reserved cash equals this595 plus3,018.120 in unused ordinary BUY drafts15465/15470/15471. The complete dispatch contains no intent/attempt/ACK for these four qids and the report contains no corresponding fill. Existing proved-unused retirement releases local reservations; it does not credit cash, establish a fresh order or waive reconciliation.

**No selector override is supported yet.** Installed `paper5` is absent. `probabilistic_portfolio.rs::optimize` charges capital opportunity cost only on gross new BUY funding; it grants no credit for pure-SELL released cash, forbids projected SELL proceeds funding gross BUYs, and still selects positive expected increment. `authority.rs::prepare_probabilistic` denies overlap with managed cohorts. `prize_utility.rs::evaluate` is offline/non-authorizing and requires all holdings in the same joint state surface, source-bound opponent terminal wealth and prize/tie rules. Current six-contract semantic coverage cannot score this larger account. Its scorer also cannot create a candidate that the expected-wealth optimizer omitted. None of these components can legitimately bypass the existing managed value hold.

The next useful decision is an owner-reviewed **same-time HOLD versus SELL-and-REDEPLOY comparison**, using existing source inputs, before adding a policy: fresh executable depth and liabilities, horizon-correct cash value (especially MA), a genuinely better funded marginal acquisition, acquisition/capital-time uncertainty, joint factor/exception risk, and complete prize/opponent evidence if rank utility is the reason to accept lower expected wealth. Current CTM points lack demonstrated joint calibration; Atlas's separate earlier-year diagnostics did not establish joint superiority and do not qualify these CTM reviews. Without that evidence, lowering a value floor merely to book positive FIFO P&L is a no-go.

## Recheck from the retained private originals

Run this stdlib-only check from the repository root with the pinned private ZIP path. It verifies pins/chains and reproduces both depth/FIFO comparisons without writing raw data or contacting a service. The aggregate additionally records exact row hashes/source clocks and review sensitivity arithmetic.

```python
import json, hashlib, zipfile
from decimal import Decimal as D
from pathlib import Path
a = json.loads(Path('research/moloch-capital-turnover-20261017/aggregate.json').read_text())
p = Path(r'C:\Users\csram\AppData\Local\Temp\moloch-source-cow-incident-r26-20261017.zip')
h = lambda b: hashlib.sha256(b).hexdigest()
assert h(p.read_bytes()) == a['input']['zipSha256']
with zipfile.ZipFile(p) as z:
    pins = json.loads(z.read('packet-manifest.json'))['files']
    for name, pin in pins.items():
        raw = z.read(name)
        assert len(raw) == pin['bytes'] and h(raw) == pin['sha256']
    anchor = json.loads(z.read('anchor-head.original.json'))
    for name, start, previous in [('native.suffix.jsonl', anchor['sequence'], anchor['previous']),
                                  ('dispatch.original.ndjson', 0, '0'*64)]:
        for line in z.read(name).splitlines():
            row = json.loads(line); digest = row['hash']
            fragment = b',"hash":"' + digest.encode() + b'"'
            assert line.count(fragment) == 1 and h(line.replace(fragment, b'', 1)) == digest
            assert row['sequence'] == start + 1 and row['previous'] == previous
            start, previous = row['sequence'], digest
        expected = a['input']['nativeChain' if name.startswith('native') else 'dispatchChain']
        assert (start, previous) == (expected['lastSequence'], expected['lastHash'])
    report = json.loads(z.read('report.original.json'))
    rows = {json.loads(line)['sequence']: (json.loads(line), line)
            for line in z.read('native.suffix.jsonl').splitlines()}
    for c in a['candidates']:
        row, raw = rows[c['bookPin']['nativeSequence']]
        assert h(raw) == c['bookPin']['rawNativeRowSha256WithoutNewline']
        b = row['event']['book']; n = c['quantity']; left = n; basis = D(0)
        for lot in report['report']['positions'][c['exchangeId']][c['side']]:
            k = min(left, lot['quantity']); basis += k*D(lot['price']); left -= k
            if not left: break
        assert left == 0
        left = n; proceeds = D(0); limit = None
        for level in b['bids']:  # Both audited slices are YES, not a general NO adapter.
            k = min(left, level['quantity']); proceeds += k*D(level['price']); left -= k
            if k: limit = D(level['price'])
            if not left: break
        assert left == 0 and c['side'] == 'YES'
        assert basis == D(c['fifoBasis']) and proceeds == D(c['visibleCurveHypotheticalProceeds'])
        assert proceeds-basis == D(c['visibleCurveHypotheticalGain'])
        assert limit*n-basis == D(c['gainIfAllFilledAtFixedLimit'])
print('original pins, both complete chains, and both FIFO/depth comparisons verified')
```
