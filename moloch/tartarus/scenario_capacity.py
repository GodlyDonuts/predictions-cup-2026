"""Scenario-specific static rotation relaxation, never an executable plan.

An oracle knows the supplied ordinary outcome and can sell losing inventory
before buying its winning outcomes. Zero fees/debt and relaxed resource, FIFO,
risk, eligibility and timing constraints make this deliberately optimistic.
Unknown/refund branches stay unbounded; this is not a future/global capacity
bound or a calibrated win probability. Modeled proceeds never credit an account.
"""
from decimal import Decimal
from .rotation_capacity import static_upper_bound

UNIT = 1_000_000


def need(ok, reason):
    if not ok:
        raise ValueError(reason)


def integer(value, minimum, maximum, reason):
    need(type(value) is int and minimum <= value <= maximum, reason)
    return value


def levels(rows, bid):
    need(type(rows) is list and len(rows) <= 10000, "CAPACITY_BOOK_DEPTH")
    out=[]
    for row in rows:
        p=integer(row.get("price_micros"),1,UNIT-1,"CAPACITY_PRICE")
        n=integer(row.get("quantity"),1,10**9,"CAPACITY_QUANTITY")
        out.append({"price_micros":p,"quantity":n})
    need(len({r["price_micros"] for r in out})==len(out),"CAPACITY_DUPLICATE_PRICE")
    return sorted(out,key=lambda r:r["price_micros"],reverse=bid)


def side_levels(book, side, buy):
    source=book["asks" if buy == (side=="YES") else "bids"]
    rows=[dict(r,price_micros=r["price_micros"] if side=="YES" else UNIT-r["price_micros"]) for r in source]
    return sorted(rows,key=lambda r:r["price_micros"],reverse=not buy)


def clipped(rows, quantity):
    out=[]
    for row in rows:
        n=min(quantity,row["quantity"])
        if n:out.append(dict(row,quantity=n));quantity-=n
        if not quantity:break
    return out


def ordinary_oracle(*, cash_micros, holdings, registry, books, scenarios):
    integer(cash_micros,0,10**18,"CAPACITY_CASH")
    need(type(registry) is list and 1<=len(registry)<=512,"CAPACITY_REGISTRY")
    ex_to_contract={}
    contracts=set()
    for r in registry:
        ex,cid=r.get("exchange_id"),r.get("contract_id")
        need(type(ex) is str and ex and type(cid) is str and cid
             and ex not in ex_to_contract and cid not in contracts,"CAPACITY_REGISTRY")
        ex_to_contract[ex]=cid;contracts.add(cid)
    need(type(holdings) is list and len(holdings)<=1024,"CAPACITY_HOLDINGS")
    seen=set()
    for h in holdings:
        ex,side=h.get("exchange_id"),h.get("side")
        need(ex_to_contract.get(ex)==h.get("contract_id") and side in ["YES","NO"]
             and (ex,side) not in seen,"CAPACITY_UNMODELED_OR_DUPLICATE_HOLDING")
        integer(h.get("quantity"),1,10**9,"CAPACITY_HELD_QUANTITY");seen.add((ex,side))
    need(type(books) is dict and set(books)<=set(ex_to_contract),"CAPACITY_BOOK_SCOPE")
    prepared={}
    for ex,book in books.items():
        b={"bids":levels(book["bids"],True),"asks":levels(book["asks"],False)}
        need(not b["bids"] or not b["asks"] or b["bids"][0]["price_micros"]<=b["asks"][0]["price_micros"],"CAPACITY_CROSSED_BOOK")
        prepared[ex]=b
    need(type(scenarios) is list and 1<=len(scenarios)<=4096,"CAPACITY_SCENARIOS")
    ids=set();out=[]
    for s in scenarios:
        ident=s.get("id");p=s.get("payouts")
        need(type(ident) is str and ident and ident not in ids and type(p) is dict
             and set(p)==contracts and all(x in ["resolved_yes","resolved_no","refund_principal"] for x in p.values()),"CAPACITY_SCENARIO_SCOPE")
        ids.add(ident)
        if s.get("kind")!="ordinary" or "refund_principal" in p.values():
            out.append({"id":ident,"state":"unbounded_exceptional_or_refund","upper_wealth_floor_micros":None})
            continue
        winning_quantity=0;losing_sale_cash=0;winning_sales=[];buys=[]
        for h in holdings:
            ex,side,n=h["exchange_id"],h["side"],h["quantity"]
            wins=p[h["contract_id"]]==("resolved_yes" if side=="YES" else "resolved_no")
            if wins:winning_quantity+=n
            if ex not in prepared:continue
            sale=clipped(side_levels(prepared[ex],side,False),n)
            if wins:winning_sales.extend(sale)
            else:losing_sale_cash+=sum(r["price_micros"]*r["quantity"] for r in sale)
        for ex,book in prepared.items():
            side="YES" if p[ex_to_contract[ex]]=="resolved_yes" else "NO"
            buys.extend(side_levels(book,side,True))
        bound=static_upper_bound(cash_micros+losing_sale_cash,winning_quantity,buys,winning_sales,allow_sales=True)
        out.append(dict(bound,id=ident,state="ordinary_oracle_relaxation",
            original_winning_held_quantity=winning_quantity,
            conditional_losing_sale_proceeds_micros=losing_sale_cash))
    return {"rows":out,"static_book_count":len(prepared),"held_sides":len(holdings),
        "modeled_sale_proceeds_are_actual_cash":False,"shared_depth_resources_relaxed":True,
        "execution_authority":False,"global_or_future_bound":False}


def first_place_upper(capacity, probability_models, field_models):
    rows=capacity["rows"];ids=[r["id"] for r in rows];n=len(rows)
    need(type(probability_models) is list and 1<=len(probability_models)<=8
         and type(field_models) is list and 1<=len(field_models)<=8,"CAPACITY_MODELS")
    result=[];seen_p=set();seen_f=set()
    for m in probability_models:
        need(type(m.get("id")) is str and m["id"] not in seen_p,"CAPACITY_MODEL_ID");seen_p.add(m["id"])
        w=m.get("weightsPpm");need(type(w) is list and len(w)==n,"CAPACITY_WEIGHT_DIMENSION")
        for x in w:integer(x,0,UNIT,"CAPACITY_WEIGHT")
        need(sum(w)==UNIT,"CAPACITY_WEIGHT_MASS")
    for f in field_models:
        need(type(f.get("id")) is str and f["id"] not in seen_f and f.get("completeField") is False,"CAPACITY_FIELD_ID");seen_f.add(f["id"])
        t=f.get("thresholds");need(type(t) is list and len(t)==n,"CAPACITY_FIELD_DIMENSION")
        targets=[]
        for x in t:
            need(type(x) is str,"CAPACITY_THRESHOLD");v=Decimal(x)*UNIT
            need(v.is_finite() and v==v.to_integral_value(),"CAPACITY_THRESHOLD");targets.append(int(v))
        for m in probability_models:
            impossible=0;unbounded=0
            for r,w,target in zip(rows,m["weightsPpm"],targets):
                if r["upper_wealth_floor_micros"] is None:unbounded+=w
                elif r["upper_wealth_floor_micros"]<=target:impossible+=w
            result.append({"probabilityModelId":m["id"],"fieldModelId":f["id"],
                "staticOracleFirstPlaceMassUpperPpm":UNIT-impossible,
                "unboundedExceptionalMassPpm":unbounded,"calibratedWinProbability":False})
    return {"scenarioIds":ids,"pairs":result,"executionAuthority":False,
            "firstPlaceIsStrict":True,"currentOrFutureGlobalBound":False}
