import hashlib,json,unittest
from types import SimpleNamespace
from .official_marks import normalize_positions,OfficialMarkTape,snapshot_official,OfficialGrowthLedger
from .contract import UNIT
from .test_actions import environment
from moloch.learning_realism.protocol import DAY,growth_score


def receipt(quantity=-3,price='.4',value='1.80'):
    return json.dumps(dict(endpoint='/tournaments/cup/portfolio/positions',status=200,
        requestStartedAt='2026-10-19T00:00:00Z',receivedAt='2026-10-19T00:00:01Z',
        data={'positions':[dict(exchangeId='2',marketId='1',quantity=quantity,currentPrice=price,marketValue=value)]})).encode()


def row(at=0,price=400000,exchange='2'):
    return dict(kind='official_mark',exchange_id=exchange,market_id='1',tournament_id='cup',
        available_at_ms=at,yes_price_micros=price,original_sha256='a'*64)


class OfficialMarksTests(unittest.TestCase):
    def test_signed_NO_value_validates_YES_mark_not_execution_quote(self):
        raw=receipt();rows=normalize_positions(raw,hashlib.sha256(raw).hexdigest(),
            endpoint='/tournaments/cup/portfolio/positions',tournament='cup',bounds=(1792368000000,1792368010000))
        self.assertEqual(rows[0]['yes_price_micros'],400000)
        tape=OfficialMarkTape(rows,tournament='cup',bounds=(1792368000000,1792368010000))
        e=SimpleNamespace(now=1792368001000,cash=100*UNIT,lots={('2','NO'):[[3,500000]]})
        self.assertEqual(snapshot_official(e,tape).twice_micros,2*(100*UNIT+1800000))
        self.assertFalse(hasattr(e,'books'))

    def test_cent_rounding_is_bounded_not_price_reorientation(self):
        for value,valid in [('.86',True),('.87',False)]:
            raw=receipt(-1,'.142799',value)
            args=dict(endpoint='/tournaments/cup/portfolio/positions',tournament='cup',bounds=(1792368000000,1792368010000))
            if valid:self.assertEqual(len(normalize_positions(raw,hashlib.sha256(raw).hexdigest(),**args)),1)
            else:
                with self.assertRaisesRegex(ValueError,'ORIENTATION'):normalize_positions(raw,hashlib.sha256(raw).hexdigest(),**args)

    def test_raw_hash_scope_and_role_are_mandatory(self):
        raw=receipt();args=dict(endpoint='/tournaments/cup/portfolio/positions',tournament='cup',bounds=(1792368000000,1792368010000))
        with self.assertRaisesRegex(ValueError,'RECEIPT_CHANGED'):normalize_positions(raw,'b'*64,**args)
        args['endpoint']='/tournaments/other/portfolio/positions'
        with self.assertRaisesRegex(ValueError,'ENDPOINT'):normalize_positions(raw,hashlib.sha256(raw).hexdigest(),**args)
        args['endpoint']='/tournaments/cup/portfolio/positions';args['bounds']=(0,100)
        with self.assertRaisesRegex(ValueError,'INFORMATION_CLOCK'):normalize_positions(raw,hashlib.sha256(raw).hexdigest(),**args)

    def test_future_and_missing_marks_are_not_midpoint_fallback(self):
        tape=OfficialMarkTape([row(10),{**row(20),'kind':'official_mark_missing'}],tournament='cup',bounds=(0,100))
        e=SimpleNamespace(now=9,cash=0,lots={('2','YES'):[[1,500000]]},books={'2':'an executable book is insufficient'})
        with self.assertRaisesRegex(ValueError,'UNAVAILABLE'):snapshot_official(e,tape)
        e.now=10;self.assertEqual(snapshot_official(e,tape).twice_micros,800000)
        e.now=20
        with self.assertRaisesRegex(ValueError,'UNAVAILABLE'):snapshot_official(e,tape)

    def test_stale_mark_is_explicit_and_zero_position_needs_no_price(self):
        tape=OfficialMarkTape([row()],tournament='cup',bounds=(0,100))
        e=SimpleNamespace(now=99,cash=0,lots={('2','YES'):[[1,500000]],('3','NO'):[]})
        with self.assertRaisesRegex(ValueError,'STALE'):snapshot_official(e,tape,maximum_age_ms=10)
        result=snapshot_official(e,tape,maximum_age_ms=10,historical_last_mark=True)
        self.assertEqual(result.stale_exchanges,('2',));self.assertIn('explicit_staleness',result.convention)

    def test_role_excludes_next_boundary_mark_but_allows_terminal_valuation(self):
        tape=OfficialMarkTape([row(0)],tournament='cup',bounds=(0,100))
        self.assertEqual(tape.get('2',100)['available_at_ms'],0)
        with self.assertRaisesRegex(ValueError,'ROLE_OR_TOURNAMENT'):
            OfficialMarkTape([row(100)],tournament='cup',bounds=(0,100))
        with self.assertRaisesRegex(ValueError,'OUTSIDE_ROLE'):tape.get('2',101)

    def test_original_human_daily_reward_runs_on_official_prices(self):
        e=environment();e.lots={('2','YES'):[[100,400000]]};e.end=e.now+2*DAY
        tape=OfficialMarkTape([row(e.now,500000),row(e.now+DAY,600000),row(e.end-1,700000)],
            tournament='cup',bounds=(0,e.end))
        ledger=OfficialGrowthLedger(e,tape,maximum_mark_age_ms=2*DAY)
        e.advance(e.now+DAY);ledger.transition();e.advance(e.end);ledger.transition();result=ledger.finish()
        expected=growth_score([(1050,1060),(1060,1070)],[('whole_competition_correlated_cluster',20)])
        self.assertAlmostEqual(result['reward'],expected['reward']);self.assertEqual(result['D'],2)
        self.assertIn('official_tournament',result['valuation']);self.assertFalse(result['production_profitability_established'])

    def test_cash_reservations_and_missing_mark_do_not_change_legal_orders(self):
        e=environment();before=e.observe()['masks']['family'].copy()
        e.lots={('2','YES'):[[1,400000]]}
        tape=OfficialMarkTape([],tournament='cup',bounds=(0,100001))
        with self.assertRaisesRegex(ValueError,'UNAVAILABLE'):snapshot_official(e,tape)
        # Missing valuation was not a portfolio projection, rejection or mask.
        self.assertTrue((before==e.observe()['masks']['family']).all())
        self.assertEqual(e.cash,1000*UNIT)

    def test_new_scope_and_same_timestamp_conflicts_are_rejected(self):
        with self.assertRaisesRegex(ValueError,'EXCHANGE_MARKET_CHANGED'):
            OfficialMarkTape([row(0),{**row(1),'market_id':'9'}],tournament='cup',bounds=(0,100))
        with self.assertRaisesRegex(ValueError,'CONFLICTING'):
            OfficialMarkTape([row(0),row(0,500000)],tournament='cup',bounds=(0,100))

    def test_original_normalized_marks_cannot_be_rewritten_through_reader(self):
        original=row();tape=OfficialMarkTape([original],tournament='cup',bounds=(0,100))
        original['yes_price_micros']=900000
        self.assertEqual(tape.get('2',0)['yes_price_micros'],400000)
        with self.assertRaises(TypeError):tape.get('2',0)['yes_price_micros']=900000
