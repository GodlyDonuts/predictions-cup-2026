"""Reference rows indexed by their existing immutable market/source keys.

Rows retain their original objects and information clocks. All mapping writes,
including inherited update/pop/clear operations, maintain the same index.
"""
from collections.abc import MutableMapping


class ReferenceCache(MutableMapping):
    def __init__(self,rows=()):
        self._rows={};self._markets={};self.update(rows)

    def __getitem__(self,key):return self._rows[key]
    def __iter__(self):return iter(self._rows)
    def __len__(self):return len(self._rows)

    def __setitem__(self,key,value):
        market,source=key
        self._rows[key]=value;self._markets.setdefault(market,{})[key]=value

    def __delitem__(self,key):
        del self._rows[key];market,source=key;bucket=self._markets[market]
        del bucket[key]
        if not bucket:del self._markets[market]

    def for_market(self,market):
        return self._markets.get(market,{}).items()
