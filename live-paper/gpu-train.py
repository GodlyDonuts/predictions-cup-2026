"""Batched regularized GPU regressors. No API access, orders, or account data."""
import argparse, datetime, itertools, json, math, os, sys, time

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--dataset', required=True); p.add_argument('--out', required=True)
    p.add_argument('--epochs', type=int, default=500); p.add_argument('--device', default='cuda')
    a = p.parse_args()
    if os.path.exists(a.out): raise ValueError('Refusing to overwrite frozen bank')
    if not 50 <= a.epochs <= 2000: raise ValueError('Epoch budget outside supported range')
    os.environ['CUBLAS_WORKSPACE_CONFIG']=':4096:8'
    import torch
    if a.device != 'cuda' or not torch.cuda.is_available(): raise ValueError('CUDA GPU required')
    torch.set_num_threads(2); torch.use_deterministic_algorithms(True)
    data = json.load(open(a.dataset, encoding='utf-8'))
    if data['config']['feeBps'] != 0: raise ValueError('Zero-fee baseline required')
    models, rejected, parity = [], [], []; started = time.monotonic()
    variants = list(itertools.product([8, 16], [0.01, 0.1], [17, 43]))
    for s in data['sets']:
        train, test = s['train'], s['test']
        split = data['developmentSplitAt']
        if any(r['labelAt'] >= split for r in train) or any(r['at'] < split for r in test): raise ValueError('Unpurged chronological split')
        for width in [4, 6, 9, 12]:
            ids = [f"gpu-mlp-h{s['horizon']}-f{width}-n{h}-wd{decay}-s{seed}" for h, decay, seed in variants]
            if len(train) < 20 or len(test) < 10 or not s['eligible']:
                rejected.extend({'id': name, 'reason': 'insufficient_supported_matured_labels', 'train': len(train), 'test': len(test)} for name in ids); continue
            # Training-only scales. Normalized intercept remains zero; bias is separate.
            tx = torch.tensor([r['x'][:width] for r in train], dtype=torch.float64, device='cuda')
            vx = torch.tensor([r['x'][:width] for r in test], dtype=torch.float64, device='cuda')
            y = torch.tensor([r['y'] for r in train], dtype=torch.float64, device='cuda')
            means, scales = tx.mean(0), tx.std(0, unbiased=False).clamp_min(1e-6)
            yy, ys = y.mean(), y.std(unbiased=False).clamp_min(1e-6)
            tx, vx, yn = (tx-means)/scales, (vx-means)/scales, (y-yy)/ys
            # Two widths are fitted in homogeneous batches; each seed has independent weights.
            for hidden in [8, 16]:
                specs = [(h,d,seed) for h,d,seed in variants if h == hidden]; count = len(specs)
                initial=[]
                for _,_,seed in specs:
                    g=torch.Generator(device='cuda').manual_seed(seed)
                    initial.append((torch.randn(width,hidden,device='cuda',dtype=torch.float64,generator=g)/math.sqrt(width),torch.randn(hidden,device='cuda',dtype=torch.float64,generator=g)/math.sqrt(hidden)))
                w1=torch.nn.Parameter(torch.stack([x[0] for x in initial])); b1=torch.nn.Parameter(torch.zeros(count,hidden,device='cuda',dtype=torch.float64))
                w2=torch.nn.Parameter(torch.stack([x[1] for x in initial])); b2=torch.nn.Parameter(torch.zeros(count,device='cuda',dtype=torch.float64))
                opt=torch.optim.Adam([w1,b1,w2,b2],lr=0.01)
                decay=torch.tensor([d for _,d,_ in specs],device='cuda',dtype=torch.float64)
                def forward(x): return (torch.relu(x.unsqueeze(0) @ w1+b1.unsqueeze(1))*w2.unsqueeze(1)).sum(2)+b2.unsqueeze(1)
                for _ in range(a.epochs):
                    opt.zero_grad(); pred=forward(tx)
                    loss=(((pred-yn.unsqueeze(0))**2).mean(1)+decay*(w1.square().mean((1,2))+w2.square().mean(1))).sum()
                    if not torch.isfinite(loss): raise ValueError('Nonfinite training loss')
                    loss.backward(); opt.step()
                with torch.no_grad():
                    values=(forward(vx)*ys+yy).cpu().tolist()
                    for k,(_,d,seed) in enumerate(specs):
                        errors=[v-r['y'] for v,r in zip(values[k],test)]
                        model={'id':f"gpu-mlp-h{s['horizon']}-f{width}-n{hidden}-wd{d}-s{seed}", 'family':'gpu_mlp','horizon':s['horizon'],'width':width,'hidden':hidden,'decay':d,'seed':seed,'epochs':a.epochs,'means':means.cpu().tolist(),'scales':scales.cpu().tolist(),'inputWeights':w1[k].cpu().tolist(),'hiddenBias':b1[k].cpu().tolist(),'outputWeights':w2[k].cpu().tolist(),'outputBias':b2[k].item(),'targetMean':yy.item(),'targetScale':ys.item(),'coverage':s['coverage'],'trainRows':len(train),'testRows':len(test),'rmse':math.sqrt(sum(e*e for e in errors)/len(errors)),'baselineRmse':math.sqrt(sum((r['y']-r['persistence'])**2 for r in test)/len(test)), 'maxTrainingLabelAt':max(r['labelAt'] for r in train)}
                        model['beatsPersistence']=model['rmse'] < model['baselineRmse'];models.append(model)
                        parity.append({'id':model['id'],'x':test[0]['x'],'expected':values[k][0]})
    selected=[]
    for h in [60,180,300,600]: selected.extend(sorted((m for m in models if m['horizon']==h),key=lambda m:(m['rmse'],m['id']))[:3])
    bank={k:v for k,v in data.items() if k != 'sets'}
    bank.update({'createdAt':datetime.datetime.now(datetime.timezone.utc).isoformat().replace('+00:00','Z'),'candidatesAttempted':128,'modelsTrained':len(models),'models':models,'selected':selected,'rejected':rejected,'labelDiagnostics':[{k:v for k,v in s.items() if k not in ['train','test']} | {'train':len(s['train']),'test':len(s['test'])} for s in data['sets']], 'selection':'Three lowest purged development RMSE per horizon; fixed 500-epoch budget, no future-data tuning','trainingRuntime':{'device':torch.cuda.get_device_name(0),'torch':torch.__version__,'seconds':time.monotonic()-started,'epochs':a.epochs,'dtype':'float64'},'inferenceParity':parity,'limitations':['Short correlated training data; no profitability established','Development model selection is not untouched validation','Conditional completed-roundtrip labels exclude censored decisions; selection bias possible','Flat-wallet training labels can differ from prospective wallet risk and cash','Only subsequent capture is admitted to independent paper wallets']})
    with open(a.out,'x',encoding='utf-8') as f: json.dump(bank,f,allow_nan=False)
    print(json.dumps({'trained':len(models),'attempted':128,'selected':len(selected),'device':bank['trainingRuntime']['device'],'seconds':bank['trainingRuntime']['seconds']}))

if __name__ == '__main__': main()
