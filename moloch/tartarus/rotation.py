"""Offline RotationPlan: independent modeled execution branches, never orders.

compare_rotation(account, scenarios, sell, branches, replacement_actions,
                 probability_models, rival_models, constraints,
                 frontier_callback=None)
uses Contract A compile_account/fifo_sell and Contract B solve_frontier/
evaluate_candidate. The callback signature is B's solve_frontier signature;
only its bounded candidates[*].quantities are accepted, then independently
rechecked by B against each branch's funded account and original fresh depth.

sell: id, contract_id, exchange_id, side, mode(immediate|passive), quantity,
available_quantity (explicit unpledged research assumption), limit_price_micros,
fee_per_share_micros, source_sha256, available_at_ms, valid_until_ms,
assumption_label. Immediate requires descending bid levels[{quantity,
unit_proceeds_micros}]; every modeled fill consumes the prefix at >=limit.
Passive requires each positive-fill branch's explicit fill_price_micros >=limit;
it never assumes fill from a quote/ACK. Fees must be supplied, never default0.

branches: at most32 {id, kind(hold|ack_no_fill|no_fill|partial_fill|full_fill),
filled_quantity, executed_at_ms (positive fills only), replacement_decision_ms,
replacement_status(search|failed|not_attempted), assumption_label}. Missing or
expired branch evidence yields explicit inconclusive, not zero return. Failed
replacement always retains the sold account/cash without hypothetical buys.
Every search additionally retains a failed-replacement alternative.

Each branch starts from the SAME original account, preserves original FIFO/source
clocks/debt/reservations and removes sold units before whole-account valuation.
Net proceeds enter modeled cash once; removed basis is accounting attribution,
not a second expense. Opportunity cost is the final wealth delta versus hold,
not another cash subtraction. No branch averaging or atomic sale/buy campaign
is asserted. All fill probabilities, calibrated odds, capital-time and actual
execution remain unavailable. Source hashes bind declarations, not authenticity.
"""
from copy import deepcopy
import hashlib
import json

try:
    from . import account_scenarios as A, target_frontier as B
except ImportError:
    import account_scenarios as A
    import target_frontier as B

UNIT = PPM = 1000000
LIMIT = 2**52


def need(value, code):
    if not value:
        raise ValueError(code)


def integer(value, code, minimum=0, maximum=LIMIT):
    need(type(value) is int and minimum <= value <= maximum, code)
    return value


def identity(value):
    return B.identifier(value)


def scoring_constraints(constraints, at):
    # The no-action comparator must still be reported when it violates a desired
    # target/floor. Candidate constraints are applied unchanged for replacements.
    result = deepcopy(constraints)
    result['decision_ms'] = at
    for field in ('minimum_wealth_micros', 'maximum_loss_micros',
                  'minimum_expected_gain_micros_by_model'):
        result.pop(field, None)
    return result


def metrics(wealth, baseline, probabilities, targets, hold_targets, accounting):
    means = {}
    for p in probabilities:
        before = sum(v*w for v,w in zip(baseline, p['weights_ppm']))
        after = sum(v*w for v,w in zip(wealth, p['weights_ppm']))
        means[p['id']] = {'before_numerator': before, 'after_numerator': after,
                          'gain_numerator': after-before, 'denominator': PPM}
    deltas = [t['strict_hit_ppm']-hold_targets[k]['strict_hit_ppm'] for k,t in targets.items()]
    gains = [m['gain_numerator'] for m in means.values()]
    rank_up, rank_down = any(x>0 for x in deltas), any(x<0 for x in deltas)
    if rank_up and min(gains)<0:
        classification = 'rank_risk_improvement_with_negative_declared_money'
    elif accounting is not None and accounting>0 and rank_down:
        classification = 'accounting_profit_with_rank_deterioration'
    elif rank_up and not rank_down:
        classification = 'declared_rank_improvement'
    elif rank_down:
        classification = 'declared_rank_deterioration'
    else:
        classification = 'no_declared_strict_rank_change'
    hits = [t['strict_hit_ppm'] for t in targets.values()]
    return {'classification': classification, 'monetary_expectations_vs_original_hold': means,
            'expected_money_gain_numerator_range': [min(gains), max(gains)],
            'expected_money_denominator': PPM,
            'strict_first_place_ppm_range_across_declared_pairs': [min(hits), max(hits)],
            'strict_first_place_delta_ppm_range': [min(deltas), max(deltas)],
            'first_place_including_ties': None, 'expected_prize': None,
            'opportunity_cost_micros_by_scenario': [a-b for a,b in zip(baseline, wealth)]}


def campaign_constraints(candidate, original, probabilities, constraints):
    """Risk/mean limits compare the whole rotation to original hold, not twice."""
    wealth = candidate['whole_account_wealth_after_micros']
    reasons = []
    if 'maximum_loss_micros' in constraints:
        loss = integer(constraints['maximum_loss_micros'], 'MAXIMUM_LOSS')
        if any(a-b < -loss for a,b in zip(wealth,original['baseline_wealth_micros'])):
            reasons.append('ROTATION_MAXIMUM_LOSS')
    if 'minimum_wealth_micros' in constraints and min(wealth)<constraints['minimum_wealth_micros']:
        reasons.append('ROTATION_WEALTH_FLOOR')
    for p in probabilities:
        threshold = constraints.get('minimum_expected_gain_micros_by_model', {}).get(p['id'])
        if threshold is not None:
            B.integer(threshold,'MEAN_THRESHOLD')
            gain = sum((a-b)*w for a,b,w in zip(wealth,original['baseline_wealth_micros'],p['weights_ppm']))
            if gain < threshold*PPM:
                reasons.append('ROTATION_MEAN_FLOOR:'+p['id'])
    candidate['rotation_declared_constraints_feasible'] = not reasons
    candidate['rotation_constraint_rejections'] = reasons


def _sale(account, sell, branch, now):
    need(type(sell) is dict and type(branch) is dict, 'SALE_OR_BRANCH_REQUIRED')
    identity(sell.get('id')); identity(branch.get('id'))
    identity(sell.get('contract_id')); identity(sell.get('exchange_id'))
    need(sell.get('side') in ('YES', 'NO'), 'SALE_SIDE')
    B.sha(sell.get('source_sha256'))
    need(isinstance(sell.get('assumption_label'), str) and sell['assumption_label'].strip(), 'SALE_ASSUMPTION')
    need(isinstance(branch.get('assumption_label'), str) and branch['assumption_label'].strip(), 'BRANCH_ASSUMPTION')
    mode = sell.get('mode')
    need(mode in ('immediate', 'passive'), 'SALE_MODE')
    kind = branch.get('kind')
    need(kind in ('hold', 'ack_no_fill', 'no_fill', 'partial_fill', 'full_fill'), 'BRANCH_KIND')
    n = integer(branch.get('filled_quantity'), 'FILLED_QUANTITY')
    requested = integer(sell.get('quantity'), 'SALE_QUANTITY', 1, 10**9)
    available = integer(sell.get('available_quantity'), 'AVAILABLE_SALE_QUANTITY', 0, 10**9)
    need(requested <= available and n <= requested, 'SALE_QUANTITY_COVERAGE')
    need((n == 0) if kind in ('hold','ack_no_fill','no_fill') else (0 < n < requested) if kind=='partial_fill' else n==requested, 'BRANCH_FILL_MISMATCH')
    matches = [h for h in account['holdings'] if (h['contract_id'], h['exchange_id'], h['side']) ==
               (sell['contract_id'], sell['exchange_id'], sell['side'])]
    need(len(matches)==1 and available<=matches[0]['quantity'], 'EXACT_OWNED_FIFO_REQUIRED')
    limit = integer(sell.get('limit_price_micros'), 'SALE_LIMIT', 0, UNIT)
    fee = integer(sell.get('fee_per_share_micros'), 'SALE_FEE_REQUIRED', 0, UNIT)
    start = integer(sell.get('available_at_ms'), 'SALE_AVAILABLE_CLOCK')
    until = integer(sell.get('valid_until_ms'), 'SALE_EXPIRY_CLOCK')
    need(start<=now<until, 'SALE_SOURCE_EXPIRED_OR_FUTURE')
    at = integer(branch.get('replacement_decision_ms'), 'REPLACEMENT_CLOCK', now)
    executed = None
    if n:
        executed = integer(branch.get('executed_at_ms'), 'EXECUTION_CLOCK', now, at)
        need(executed<until, 'MODELED_EXECUTION_AFTER_SALE_EXPIRY')
    gross = 0
    if mode == 'immediate':
        levels = sell.get('levels')
        need(isinstance(levels,list) and 1<=len(levels)<=64, 'SELL_DEPTH_REQUIRED')
        remaining = n
        previous = UNIT+1
        for level in levels:
            q = integer(level.get('quantity'), 'SELL_LEVEL_QUANTITY', 1, 10**9)
            price = integer(level.get('unit_proceeds_micros'), 'SELL_LEVEL_PRICE', 0, UNIT)
            need(price<=previous, 'SELL_DEPTH_NOT_DESCENDING'); previous=price
            take = min(remaining,q) if price>=limit else 0
            gross += take*price; remaining-=take
        need(remaining==0, 'SALE_EXCEEDS_EXECUTABLE_PREFIX')
    elif n:
        price = integer(branch.get('fill_price_micros'), 'PASSIVE_FILL_PRICE_REQUIRED', limit, UNIT)
        gross = n*price
    fees = n*fee
    integer(gross, 'SALE_GROSS_RANGE'); integer(fees, 'SALE_FEE_RANGE')
    fifo = A.fifo_sell(matches[0], n)
    after = deepcopy(account)
    held = next(h for h in after['holdings'] if (h['contract_id'],h['exchange_id'],h['side']) ==
                (sell['contract_id'],sell['exchange_id'],sell['side']))
    held['quantity'], held['lots'] = fifo['remaining_quantity'], fifo['remaining_lots']
    after['holdings'] = [h for h in after['holdings'] if h['quantity']>0]
    after['cash_micros'] = integer(account['cash_micros']+gross-fees, 'POST_SALE_CASH')
    need(after['cash_micros']>=after['reserved_cash_micros'], 'SALE_FEES_NOT_FUNDED')
    after['evidence_class'] = 'synthetic' if account['evidence_class']=='synthetic' else 'modeled_rotation'
    proof = {'filled_quantity': n, 'requested_quantity': requested, 'gross_proceeds_micros': gross,
             'fees_micros': fees, 'net_cash_change_micros': gross-fees,
             'removed_fifo_basis_micros': fifo['removed_basis_micros'],
             'accounting_realized_gain_micros': gross-fees-fifo['removed_basis_micros'],
             'remaining_fifo_quantity': fifo['remaining_quantity'], 'remaining_fifo_lots': fifo['remaining_lots'],
             'cash_before_micros': account['cash_micros'], 'cash_after_sale_micros': after['cash_micros'],
             'original_holding_source_sha256': matches[0]['source_sha256'],
             'sale_source_sha256': sell['source_sha256'], 'sale_available_at_ms': start,
             'sale_valid_until_ms': until, 'modeled_executed_at_ms': executed,
             'mode': mode, 'assumption_label': branch['assumption_label'],
             'cash_transition_is_modeled_not_actual': True, 'execution_authorized': False}
    return after, proof, at


def compare_rotation(account, scenarios, sell, branches, replacement_actions,
                     probability_models, rival_models, constraints, frontier_callback=None):
    """Return separate full-account branches, never averaged hypothetical cash."""
    need(isinstance(constraints,dict), 'CONSTRAINTS_REQUIRED')
    now = integer(constraints.get('decision_ms'), 'DECISION_CLOCK')
    need(isinstance(branches,list) and 1<=len(branches)<=32, 'BRANCH_BOUND')
    ids = [identity(b.get('id')) for b in branches]
    need(len(set(ids))==len(ids), 'DUPLICATE_BRANCH')
    original = A.compile_account(account, scenarios, now)
    try:
        if 'maximum_loss_micros' in constraints:
            integer(constraints['maximum_loss_micros'], 'MAXIMUM_LOSS')
        if 'minimum_wealth_micros' in constraints:
            B.integer(constraints['minimum_wealth_micros'], 'MINIMUM_WEALTH')
        means = constraints.get('minimum_expected_gain_micros_by_model', {})
        need(isinstance(means,dict) and set(means)<={p['id'] for p in probability_models}, 'MEAN_CONSTRAINT_MODEL')
        for value in means.values():
            B.integer(value,'MEAN_THRESHOLD')
        hold = B.evaluate_candidate(original, [], probability_models, rival_models,
                                    scoring_constraints(constraints,now), {})
    except (ValueError,KeyError) as e:
        return {'status':'inconclusive','reason':str(e),'branches':[],
                'execution_authorized':False,'calibrated_probability_claimed':False}
    hold['rotation_comparison'] = metrics(original['baseline_wealth_micros'], original['baseline_wealth_micros'],
                                         probability_models,hold['targets'],hold['targets'],None)
    out = {'schema_version':1,'status':'declared_rotation_branches','baseline':hold,'branches':[],
           'scenario_ids':original['scenario_ids'],'original_account_source_sha256':account['source_sha256'],
           'original_scenario_source_sha256':scenarios['source_sha256'],
           'original_as_of_ms':original['as_of_ms'],'original_valid_until_ms':original['valid_until_ms'],
           'original_compiled_sha256':original['source_sha256'],'branch_probabilities_ppm':None,
           'assumptions':{'probability_models':{p['id']:p['assumption_label'] for p in probability_models},
                          'rival_models':{r['id']:r['assumption_label'] for r in rival_models},
                          'replacement_fill':'Each candidate assumes only its declared acquisition quantity fills; failure is retained separately.',
                          'source_authentication':'Byte/source declarations are not independently authenticated here.'},
           'atomic_campaign_assumed':False,'execution_authorized':False,'calibrated_probability_claimed':False}
    callback = frontier_callback or B.solve_frontier
    for branch in branches:
        row = {'id':branch['id'],'kind':branch.get('kind'),'status':'inconclusive','alternatives':[],
               'execution_authorized':False,'calibrated_probability_claimed':False}
        out['branches'].append(row)
        try:
            after, proof, at = _sale(account,sell,branch,now)
            row['sale'] = proof
            compiled = A.compile_account(after,scenarios,at)
            sale_only = B.evaluate_candidate(compiled,[],probability_models,rival_models,scoring_constraints(constraints,at),{})
            sale_only['rotation_comparison'] = metrics(compiled['baseline_wealth_micros'],original['baseline_wealth_micros'],
                                                       probability_models,sale_only['targets'],hold['targets'],proof['accounting_realized_gain_micros'])
            campaign_constraints(sale_only,original,probability_models,constraints)
            row.update(status='modeled_sale_and_failure_retained',sale_only=sale_only,
                       failed_replacement={'whole_account_wealth_micros':compiled['baseline_wealth_micros'],
                                           'cash_micros':compiled['cash_micros'], 'replacement_filled_quantity':0,
                                           'assumption_label':'Replacement fails or receives no fills after the declared sale.',
                                           'execution_authorized':False})
            status = branch.get('replacement_status')
            need(status in ('search','failed','not_attempted'), 'REPLACEMENT_STATUS_REQUIRED')
            row['replacement_status'] = status
            if status != 'search':
                continue
            # Relative gain/loss limits must not be rebased to the sold account.
            # Search broadly, then apply them to each exact whole-campaign result.
            current = scoring_constraints(constraints,at)
            B.prepare(compiled,replacement_actions,probability_models,rival_models,current)
            result = callback(deepcopy(compiled),deepcopy(replacement_actions),deepcopy(probability_models),
                              deepcopy(rival_models),deepcopy(current))
            need(isinstance(result,dict) and isinstance(result.get('candidates'),list) and
                 len(result['candidates'])<=256, 'FRONTIER_CALLBACK_RESULT_BOUND')
            row['frontier_status'] = result.get('status')
            for proposed in result['candidates']:
                checked = B.evaluate_candidate(compiled,replacement_actions,probability_models,rival_models,
                                               current,proposed.get('quantities'))
                checked['rotation_comparison'] = metrics(checked['whole_account_wealth_after_micros'],original['baseline_wealth_micros'],
                                                          probability_models,checked['targets'],hold['targets'],proof['accounting_realized_gain_micros'])
                campaign_constraints(checked,original,probability_models,constraints)
                checked['sale_confirmation_dependencies'] = ['This branch assumes its declared sale quantity actually executes before replacement.']
                row['alternatives'].append(checked)
            row['status']='inconclusive' if result.get('status')=='unavailable' else 'replacement_candidates_exactly_rechecked'
            if result.get('status')=='unavailable':
                row['reason']=result.get('unavailable_reason','FRONTIER_UNAVAILABLE')
        except (ValueError,KeyError,TypeError) as e:
            row['status']='inconclusive';row['reason']=str(e)
    out['inconclusive'] = any(b['status']=='inconclusive' for b in out['branches'])
    return out
