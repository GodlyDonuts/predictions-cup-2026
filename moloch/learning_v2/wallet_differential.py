"""Compare exact reference receipts with the deployed native wallet semantics."""
import argparse
import json
from pathlib import Path
from .account import Limits, Target, UNIT
from .clock import BudgetConfig
from .reference_env import Mechanics, ReferenceEnv, Order


def fixture():
    m=Mechanics(BudgetConfig(100,600,24,2500,'a'*64),Limits(10000,1000*UNIT,1000*UNIT,fee_per_share=1000),100,100,100,10000,20000,0)
    e=ReferenceEnv(m,0,1000*UNIT);rows=[]
    def record(label):
        a=e.account();rows.append({'label':label,'cash':str(a.cash),'reserved':str(a.reserved_cash),
                                  'owned_cost':str(a.owned_cost),'realized':str(e.realized),'fees':str(e.fees),
                                  'yes':a.quantities.get(('x','YES'),0),'no':a.quantities.get(('x','NO'),0)})
    def reserve(qid,side,n,p):
        e.orders[qid]=Order(qid,Target('x',side,UNIT,UNIT,p,'BUY'),n,n,1000000,'resting')
    def fill(qid,n,p):
        e.orders[qid].remaining-=n;e._fill_receipt(qid,e.now,[(p,n)])
    record('initial');reserve(1,'YES',10,400000);record('reserve_yes')
    fill(1,4,390000);record('partial_yes');fill(1,6,400000);record('complete_yes')
    reserve(2,'NO',6,550000);record('reserve_opposite');fill(2,3,540000);record('partial_opposite_net')
    e.orders.pop(2);record('cancel_remainder')
    e.orders[3]=Order(3,Target('x','YES',0,UNIT,600000,'SELL'),0,2,1000000,'resting')
    e._fill_receipt(3,e.now,[(600000,2)]);record('fifo_sale')
    reserve(4,'NO',5,500000);record('reserve_flatten');fill(4,5,500000);record('complete_flatten')
    return rows


def main():
    p=argparse.ArgumentParser();p.add_argument('--native-result',type=Path,required=True);args=p.parse_args()
    native=json.loads(args.native_result.read_text())['rows'];reference=fixture()
    if native!=reference: raise ValueError('NATIVE_WALLET_DIFFERENTIAL_MISMATCH')
    print(json.dumps({'classification':'offline_wallet_semantics_not_execution_calibration',
                      'matched_exact_states':len(reference),'rows':reference,'production_changed':False}))


if __name__=='__main__':main()
