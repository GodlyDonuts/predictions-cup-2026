"""Synthetic independent-branch economics; no actual source/account/fill claims."""
from copy import deepcopy
import unittest
try:
    from . import rotation as R
except ImportError:
    import rotation as R

U = 1000000
H = 'a'*64


def fixture(cost=800000, cash=0):
    e = {'schema_version':1,'evidence_class':'synthetic','as_of_ms':1000,
         'valid_until_ms':5000,'source_sha256':H,
         'unmodeled_state_statement':'Synthetic two-contract joint states, not exhaustive.'}
    account = {**e,'cash_micros':cash,'reserved_cash_micros':0,'terminal_debt_micros':0,
               'pending_obligations':[], 'holdings':[{'contract_id':'old','exchange_id':'xold','side':'YES',
               'quantity':10,'lots':[{'quantity':10,'unit_cost_micros':cost}],'source_sha256':H}]}
    scenarios = {**e,'contracts':[{'contract_id':c,'exchange_id':'x'+c,'semantic_source_sha256':H} for c in ['old','new']],
                 'scenarios':[{'id':'old_wins','kind':'ordinary','payouts':{'old':'resolved_yes','new':'resolved_no'},'assumption_label':'Synthetic old wins'},
                              {'id':'new_wins','kind':'ordinary','payouts':{'old':'resolved_no','new':'resolved_yes'},'assumption_label':'Synthetic new wins'}]}
    sale = {'id':'sell','contract_id':'old','exchange_id':'xold','side':'YES','mode':'immediate',
            'quantity':10,'available_quantity':10,'limit_price_micros':700000,'fee_per_share_micros':0,
            'levels':[{'quantity':10,'unit_proceeds_micros':700000}], 'available_at_ms':1000,'valid_until_ms':4000,
            'source_sha256':H,'assumption_label':'Synthetic unpledged exact bid depth, not actual fill.'}
    actions = [{'id':'buy_new','contract_id':'new','exchange_id':'xnew','side':'YES',
                'levels':[{'quantity':14,'unit_cost_micros':500000}],'fee_per_share_micros':0,
                'source_sha256':H,'available_at_ms':1200,'valid_until_ms':3500}]
    models = [{'id':'p','weights_ppm':[900000,100000],'assumption_label':'Synthetic toy probabilities only'}]
    rivals = [{'id':'r','thresholds_micros':[12000000,12000000],'complete_field':False,'assumption_label':'Synthetic terminal maximum'}]
    constraints = {'decision_ms':1500,'maximum_gross_cost_micros':20000000,'maximum_total_quantity':20,
                   'target_pairs':[{'probability_model_id':'p','rival_model_id':'r'}]}
    return account,scenarios,sale,actions,models,rivals,constraints


def branch(kind='full_fill', n=10, **updates):
    b = {'id':kind,'kind':kind,'filled_quantity':n,'executed_at_ms':1600,
         'replacement_decision_ms':1700,'replacement_status':'search',
         'assumption_label':'Synthetic execution branch, not an actual confirmation.'}
    b.update(updates)
    return b


def proposed(n):
    return lambda *args: {'status':'synthetic_bounded_candidates', 'candidates':[{'quantities':{'buy_new':n}}]}


def run(f, branches, callback=None):
    a,s,sell,actions,p,r,c = f
    return R.compare_rotation(a,s,sell,branches,actions,p,r,c,callback or proposed(14))


class RotationTests(unittest.TestCase):
    def test_accounting_loss_can_improve_rank_but_not_claim_alpha(self):
        out = run(fixture(),[branch()]);row=out['branches'][0];candidate=row['alternatives'][0]
        self.assertEqual(row['sale']['accounting_realized_gain_micros'],-U)
        self.assertEqual(row['sale']['cash_after_sale_micros'],7*U)
        self.assertEqual(candidate['whole_account_wealth_after_micros'],[0,14*U])
        self.assertEqual(candidate['rotation_comparison']['classification'],'rank_risk_improvement_with_negative_declared_money')
        self.assertEqual(candidate['rotation_comparison']['strict_first_place_ppm_range_across_declared_pairs'],[100000,100000])
        self.assertLess(candidate['rotation_comparison']['monetary_expectations_vs_original_hold']['p']['gain_numerator'],0)
        self.assertFalse(candidate['execution_authorized']);self.assertFalse(out['atomic_campaign_assumed'])

    def test_accounting_profit_can_harm_rank_and_proceeds_are_not_gain(self):
        f=fixture(cost=200000);f[5][0]['thresholds_micros']=[9*U,9*U]
        out=run(f,[branch(replacement_status='not_attempted')]);row=out['branches'][0]
        self.assertEqual(row['sale']['gross_proceeds_micros'],7*U)
        self.assertEqual(row['sale']['accounting_realized_gain_micros'],5*U)
        self.assertEqual(row['sale']['accounting_basis'],'remaining_fifo_principal_plus_declared_sale_fee')
        self.assertIsNone(row['sale']['historical_acquisition_fees_micros'])
        self.assertFalse(row['sale']['accounting_realized_gain_is_all_fee_net'])
        self.assertEqual(row['sale_only']['whole_account_wealth_after_micros'],[7*U,7*U])
        self.assertEqual(row['sale_only']['rotation_comparison']['classification'],'accounting_profit_with_rank_deterioration')

    def test_ack_no_fill_never_funds_replacement_and_branches_do_not_share_cash(self):
        out=run(fixture(),[branch(),branch('ack_no_fill',0)])
        self.assertEqual(out['branches'][0]['alternatives'][0]['gross_cost_micros'],7*U)
        no=out['branches'][1]
        self.assertEqual(no['sale']['cash_after_sale_micros'],0)
        self.assertEqual(no['status'],'inconclusive')
        self.assertIn('FUNDING',no['reason'])
        self.assertEqual(no['failed_replacement']['whole_account_wealth_micros'],[10*U,0])

    def test_no_fill_can_spend_original_cash_only(self):
        out=run(fixture(cash=U),[branch('no_fill',0)],proposed(2))
        row=out['branches'][0]
        self.assertEqual(row['sale']['net_cash_change_micros'],0)
        self.assertEqual(row['alternatives'][0]['gross_cost_micros'],U)
        self.assertEqual(row['alternatives'][0]['cash_after_micros'],0)

    def test_partial_fifo_refund_uses_remaining_basis_and_fee_once(self):
        f=fixture();f[0]['holdings'][0]['lots']=[{'quantity':4,'unit_cost_micros':800000},{'quantity':6,'unit_cost_micros':200000}]
        f[1]['scenarios'].append({'id':'refund','kind':'exceptional','payouts':{'old':'refund_principal','new':'refund_principal'},'assumption_label':'Synthetic principal refund'})
        f[4][0]['weights_ppm']=[800000,100000,100000];f[5][0]['thresholds_micros'].append(12*U)
        f[2]['fee_per_share_micros']=10000
        row=run(f,[branch('partial_fill',5,replacement_status='failed')])['branches'][0]
        self.assertEqual(row['sale']['removed_fifo_basis_micros'],3400000)
        self.assertEqual(row['sale']['remaining_fifo_lots'],[{'quantity':5,'unit_cost_micros':200000}])
        self.assertEqual(row['sale']['cash_after_sale_micros'],3450000)
        self.assertEqual(row['sale_only']['whole_account_wealth_after_micros'],[8450000,3450000,4450000])
        self.assertEqual(row['failed_replacement']['cash_micros'],3450000)

    def test_depth_prefix_marginal_limit_not_top_price_for_whole_sale(self):
        f=fixture();f[2]['levels']=[{'quantity':4,'unit_proceeds_micros':800000},{'quantity':6,'unit_proceeds_micros':700000}]
        row=run(f,[branch(replacement_status='failed')])['branches'][0]
        self.assertEqual(row['sale']['gross_proceeds_micros'],7400000)
        f[2]['limit_price_micros']=750000
        self.assertIn('SALE_EXCEEDS_EXECUTABLE_PREFIX',run(f,[branch()])['branches'][0]['reason'])

    def test_passive_zero_partial_and_full_are_explicit_not_ack_fills(self):
        f=fixture();f[2]['mode']='passive'
        out=run(f,[branch('no_fill',0,replacement_status='failed'),branch('partial_fill',3,fill_price_micros=720000,replacement_status='failed'),branch(fill_price_micros=710000,replacement_status='failed')])
        self.assertEqual([r['sale']['cash_after_sale_micros'] for r in out['branches']],[0,2160000,7100000])
        missing=run(f,[branch()])['branches'][0]
        self.assertIn('PASSIVE_FILL_PRICE_REQUIRED',missing['reason'])

    def test_lost_replacement_retains_sold_cash_not_hypothetical_asset(self):
        row=run(fixture(),[branch(replacement_status='failed')])['branches'][0]
        self.assertEqual(row['alternatives'],[])
        self.assertEqual(row['failed_replacement']['whole_account_wealth_micros'],[7*U,7*U])
        self.assertEqual(row['failed_replacement']['replacement_filled_quantity'],0)
        self.assertNotEqual(row['failed_replacement']['whole_account_wealth_micros'],[0,14*U])

    def test_stale_replacement_keeps_failed_branch_and_original_clocks(self):
        f=fixture();f[3][0]['valid_until_ms']=1650;before=deepcopy(f)
        row=run(f,[branch()])['branches'][0]
        self.assertIn('ACTION_EXPIRED_OR_FUTURE',row['reason'])
        self.assertEqual(row['failed_replacement']['cash_micros'],7*U)
        self.assertEqual(f,before)
        self.assertEqual(row['sale']['sale_available_at_ms'],1000)

    def test_cash_reservation_debt_and_original_hold_opportunity_cost_once(self):
        f=fixture(cash=3*U);f[0]['reserved_cash_micros']=2*U;f[0]['terminal_debt_micros']=U
        row=run(f,[branch(replacement_status='failed')])['branches'][0]
        self.assertEqual(row['sale']['cash_after_sale_micros'],10*U)
        self.assertEqual(row['sale_only']['whole_account_wealth_after_micros'],[9*U,9*U])
        self.assertEqual(row['sale_only']['rotation_comparison']['opportunity_cost_micros_by_scenario'],[3*U,-7*U])

    def test_campaign_loss_limit_not_reset_after_sale(self):
        f=fixture();f[6]['maximum_loss_micros']=4*U
        row=run(f,[branch()])['branches'][0]
        self.assertFalse(row['alternatives'][0]['rotation_declared_constraints_feasible'])
        self.assertIn('ROTATION_MAXIMUM_LOSS',row['alternatives'][0]['rotation_constraint_rejections'])
        self.assertTrue(row['sale_only']['rotation_declared_constraints_feasible'])

    def test_callback_cannot_supply_forged_wealth_or_excess_quantity(self):
        fake=lambda *args:{'candidates':[{'quantities':{'buy_new':14},'whole_account_wealth_after_micros':[999,999]}]}
        row=run(fixture(),[branch()],fake)['branches'][0]
        self.assertEqual(row['alternatives'][0]['whole_account_wealth_after_micros'],[0,14*U])
        row=run(fixture(),[branch()],proposed(15))['branches'][0]
        self.assertIn('EXCEEDS_ORIGINAL_DEPTH',row['reason'])

    def test_unknown_fee_pending_obligation_bad_chronology_and_float_quantity(self):
        f=fixture();del f[2]['fee_per_share_micros']
        self.assertIn('SALE_FEE_REQUIRED',run(f,[branch()])['branches'][0]['reason'])
        f=fixture();f[0]['pending_obligations']=[{'unknown':'order'}]
        with self.assertRaisesRegex(ValueError,'PENDING_OBLIGATIONS_UNAVAILABLE'):run(f,[branch()])
        self.assertIn('EXECUTION_CLOCK',run(fixture(),[branch(executed_at_ms=1400)])['branches'][0]['reason'])
        self.assertIn('FILLED_QUANTITY',run(fixture(),[branch(n=1.5)])['branches'][0]['reason'])

    def test_missing_probabilities_inconclusive_not_zero_and_solver_unavailable_retained(self):
        f=fixture();f[4].clear()
        out=run(f,[branch()]);self.assertEqual(out['status'],'inconclusive');self.assertEqual(out['branches'],[])
        cb=lambda *args:{'status':'unavailable','unavailable_reason':'NO_SOLVER','candidates':[]}
        row=run(fixture(),[branch()],cb)['branches'][0]
        self.assertEqual(row['status'],'inconclusive');self.assertEqual(row['reason'],'NO_SOLVER')
        self.assertEqual(row['failed_replacement']['cash_micros'],7*U)


if __name__=='__main__':
    unittest.main()
