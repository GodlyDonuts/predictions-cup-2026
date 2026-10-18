"""Exact fractional static-depth capacity relaxation; never an executable plan.

Every retained/new share optimistically pays one, even when mutually exclusive.
Zero fees/debt, released reservations, fractional shares and no mandate/market/
order/timing limits are deliberate relaxations. Shared book resources and
opposite-side netting are also relaxed. Caller supplies a dated depth census;
missing/invalidated books are NOT empty markets. This cannot bound future
replenishment, repeated trading gains, settlements or omitted instruments.

Sell levels must describe disjoint portions of ORIGINAL owned quantity, already
clipped to owned side and displayed bid depth by the caller. Their proceeds only
fund conditional modeled purchases; this module never credits an account.
"""
from fractions import Fraction

UNIT = 1_000_000


def _integer(value, minimum, maximum, reason):
    if type(value) is not int or not minimum <= value <= maximum:
        raise ValueError(reason)
    return value


def _levels(values):
    if type(values) is not list or len(values) > 65536:
        raise ValueError("CAPACITY_DEPTH_BOUND")
    out = []
    for row in values:
        if type(row) is not dict:
            raise ValueError("CAPACITY_LEVEL")
        price = _integer(row.get("price_micros"), 1, UNIT - 1, "CAPACITY_PRICE")
        quantity = _integer(row.get("quantity"), 1, 10**9, "CAPACITY_QUANTITY")
        out.append([Fraction(price, UNIT), Fraction(quantity)])
    return out


def static_upper_bound(cash_micros, held_quantity, buy_levels, sell_levels, *, allow_sales):
    """Return an exact upper for ONLY the supplied static relaxed opportunity set.

    Prices and initial cash are integer millionths; quantities are integers.
    Initial cash funds cheapest purchases first. Additional funding sells the
    highest bids first, while a purchase's unit price remains below that bid.
    Each modeled sale sacrifices its one-unit terminal payout exactly once.
    Fractional knapsack slopes prove this ordering optimal for this relaxation:
    buyer gain/cash=(1-p)/p; seller opportunity loss/cash=(1-b)/b.
    No probability model or nominal target is needed to establish capacity.
    """
    _integer(cash_micros, 0, 10**18, "CAPACITY_CASH")
    _integer(held_quantity, 0, 10**12, "CAPACITY_HELD_QUANTITY")
    if type(allow_sales) is not bool:
        raise ValueError("CAPACITY_SALE_MODE")
    buys = sorted(_levels(buy_levels), key=lambda row: row[0])
    sells = sorted(_levels(sell_levels), key=lambda row: row[0], reverse=True)
    if sum(row[1] for row in sells) > held_quantity:
        raise ValueError("CAPACITY_SALES_EXCEED_ORIGINAL_HOLDINGS")
    cash = Fraction(cash_micros, UNIT)
    wealth = cash + held_quantity
    bought = sold = Fraction(0)
    seller = 0
    for price, remaining in buys:
        spend = min(cash, price * remaining)
        quantity = spend / price
        cash -= spend
        remaining -= quantity
        bought += quantity
        wealth += quantity - spend
        if not allow_sales:
            continue
        while remaining and seller < len(sells):
            bid, available = sells[seller]
            if bid <= price:
                break
            funding = min(bid * available, price * remaining)
            sale_quantity = funding / bid
            purchase_quantity = funding / price
            wealth += purchase_quantity - sale_quantity
            bought += purchase_quantity
            sold += sale_quantity
            remaining -= purchase_quantity
            sells[seller][1] -= sale_quantity
            if sells[seller][1] == 0:
                seller += 1
    upper_micros = wealth * UNIT
    return {
        "classification": "static_captured_depth_fractional_relaxation",
        "upper_wealth_micros_numerator": upper_micros.numerator,
        "upper_wealth_micros_denominator": upper_micros.denominator,
        "upper_wealth_floor_micros": upper_micros.numerator // upper_micros.denominator,
        "fractional_bought_quantity": str(bought),
        "fractional_sold_quantity": str(sold),
        "sales_permitted_in_model": allow_sales,
        "actual_sale_proceeds_credited": False,
        "global_capacity_bound": False,
        "joint_outcome_attainability_verified": False,
        "execution_authority": False,
    }
