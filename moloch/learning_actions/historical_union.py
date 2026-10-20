"""One causal information query across separately pinned historical indexes.

Each child retains its original receipts and publication frontier. Joining
storage partitions creates neither a new venue request nor executable depth.
"""


class KalshiUnion:
    def __init__(self,readers):
        self.readers=readers;self.tickers=set().union(*(r.tickers for r in readers))

    def page(self,ticker,now,*,from_ms=None,to_ms=None,limit=1000):
        if type(limit)is not int or not 1<=limit<=1000:raise ValueError('VALID_KALSHI_HISTORY_QUERY')
        if ticker not in self.tickers:raise KeyError(ticker)
        facts={}
        for reader in self.readers:
            if ticker not in reader.tickers:continue
            for row in reader.page(ticker,now,from_ms=from_ms,to_ms=to_ms,limit=limit):
                key=row['trade']['trade_id'];old=facts.get(key)
                if old is not None:
                    if any(old[k]!=row[k]for k in('trade','source_at_ms','simulated_available_at_ms')):
                        raise ValueError('HISTORICAL_KALSHI_PARTITION_FACT_CONFLICT')
                    if row['original_receipt_sha256']>=old['original_receipt_sha256']:continue
                facts[key]=row
        # A row outside a child's newest `limit` cannot enter the global newest
        # `limit`: that child alone already has at least `limit` newer facts.
        return sorted(facts.values(),key=lambda r:(-r['source_at_ms'],r['trade']['trade_id']))[:limit]

    def close(self):
        for reader in self.readers:reader.close()


class SampleUnion:
    def __init__(self,readers):self.readers=readers

    def at(self,asset,now):
        rows=[row for r in self.readers if(row:=r.at(asset,now))is not None]
        if not rows:return None
        newest=max(r['source_at_ms']for r in rows)
        latest=[r for r in rows if r['source_at_ms']==newest]
        if any(r['price']!=latest[0]['price']or r['simulated_available_at_ms']!=latest[0]['simulated_available_at_ms']for r in latest):
            raise ValueError('HISTORICAL_SAMPLE_PARTITION_FACT_CONFLICT')
        return min(latest,key=lambda r:r['original_receipt_sha256'])

    def close(self):
        for reader in self.readers:reader.close()


def open_indexes(cls,venue,spec,*,bounds,delivery):
    readers=[]
    try:
        for source in[spec,*spec.get('additional_indexes',[])]:
            readers.append(cls(source['path'],source['manifest_sha256'],scoring_bounds=bounds,
                information_start_ms=spec['information_start_ms'],delivery=delivery))
        if len(readers)==1:return readers[0]
        return {'Kalshi':KalshiUnion,'Polymarket':SampleUnion}[venue](readers)
    except BaseException:
        for reader in readers:reader.close()
        raise
