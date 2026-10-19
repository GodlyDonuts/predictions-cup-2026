import unittest
import torch
from .market_memory_policy import MarketMemoryPolicy


def masks(T,B,N):
    return {'market':torch.ones(T,B,N,dtype=torch.bool),'family':torch.ones(T,B,N,5,dtype=torch.bool),
            'side':torch.ones(T,B,N,5,2,dtype=torch.bool),'price':torch.ones(T,B,N,5,2,5,dtype=torch.bool),
            'patience':torch.ones(T,B,N,5,2,3,dtype=torch.bool)}


class MarketMemoryTests(unittest.TestCase):
    def test_fused_sequences_match_live_steps_with_new_markets_and_async_resets(self):
        torch.manual_seed(551);torch.set_num_threads(2);model=MarketMemoryPolicy(3,hidden_size=32,market_hidden_size=8)
        T,B,N=5,2,3;x=torch.randn(T,B,N,24);g=torch.randn(T,B,14);present=torch.ones(T,B,N,dtype=torch.bool)
        present[0,:,1:]=False;present[1,:,2]=False;present[3,1,1]=False
        reset=torch.zeros(T,B,dtype=torch.bool);reset[0]=True;reset[3,0]=True;ms=masks(T,B,N)
        old=[];h=model.initial_state(B)
        for t in range(T):
            row=model.step(x[t],g[t],present[t],{k:v[t]for k,v in ms.items()},h,reset[t],deterministic=True);old.append(row);h=row['hidden']
        actions={k:torch.stack([r['actions'][k]for r in old])for k in old[0]['actions']};new=model.sequence(x,g,present,ms,reset,actions)
        for a,b in zip(old,new):
            for name in ['logprob','value','hidden']:self.assertTrue(torch.allclose(a[name],b[name],atol=2e-5),name)
        changed=x.clone();changed[-1]+=100;future=model.sequence(changed,g,present,ms,reset,actions)
        self.assertTrue(torch.equal(new[0]['value'],future[0]['value']))
        sum(r['value'].sum()+r['logprob'].sum()for r in new).backward()
        self.assertTrue(torch.isfinite(model.market_recurrent.weight_ih_l0.grad).all())
        self.assertGreater(float(model.market_recurrent.weight_ih_l0.grad.abs().sum()),0)

    def test_absent_memory_stays_zero_and_new_market_cannot_inherit_another_market(self):
        torch.manual_seed(552);model=MarketMemoryPolicy(3,hidden_size=32,market_hidden_size=8)
        x=torch.randn(1,3,24);g=torch.randn(1,14);p=torch.tensor([[True,False,False]]);ms={k:v[0]for k,v in masks(1,1,3).items()};h=model.initial_state(1)
        h[:,32:32+24]=999
        out=model.step(x,g,p,ms,h,torch.tensor([False]),deterministic=True)
        _,memory,seen=model._parts(out['hidden']);self.assertTrue(torch.equal(memory[:,1:],torch.zeros_like(memory[:,1:])))
        self.assertEqual(seen.tolist(),p.tolist())

    def test_same_quote_can_retain_distinct_market_histories(self):
        torch.manual_seed(553);model=MarketMemoryPolicy(2,hidden_size=32,market_hidden_size=8)
        x=torch.zeros(1,2,24);changed=x.clone();changed[:,1,0]=1;g=torch.zeros(1,14);p=torch.ones(1,2,dtype=torch.bool);ms={k:v[0]for k,v in masks(1,1,2).items()}
        a=model.step(x,g,p,ms,model.initial_state(1),torch.tensor([True]),deterministic=True)
        b=model.step(changed,g,p,ms,model.initial_state(1),torch.tensor([True]),deterministic=True)
        a=model.step(x,g,p,ms,a['hidden'],torch.tensor([False]),deterministic=True)
        b=model.step(x,g,p,ms,b['hidden'],torch.tensor([False]),deterministic=True)
        _,am,_=model._parts(a['hidden']);_,bm,_=model._parts(b['hidden'])
        self.assertTrue(torch.equal(am[:,0],bm[:,0]));self.assertFalse(torch.equal(am[:,1],bm[:,1]))


if __name__=='__main__':unittest.main()
