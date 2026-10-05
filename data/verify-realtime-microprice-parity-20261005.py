import json
import importlib.util
spec=importlib.util.spec_from_file_location('microprice_training','live-paper/train-realtime-microprice.py')
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
predict=module.predict
bank=json.load(open('data/realtime-microprice-model-v2-20261005.json'))
data=json.load(open('data/realtime-microprice-dataset-v2-20261005.json'))
indices=[i*(len(data['rows'])-1)//31 for i in range(32)]
rows=[data['rows'][i] for i in indices]
predictions=predict(bank['model'],rows)
with open('data/realtime-microprice-parity-cases-20261005.json','x') as f:
    json.dump([{'x':r['x'],'expected':p.tolist()} for r,p in zip(rows,predictions)],f)
