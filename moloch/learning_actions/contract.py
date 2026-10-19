"""Participant API contract observed from official public docs, 2026-10-19.

Source https://sig.thesuper.market/api/v1/docs, embedded OpenAPI SHA256
a7d5b76ce9c79afdb8ad327bd606d0b8d1287904dd690dd1a0a1009c02a5092d.
No production configuration is changed by these training-only mechanics.
"""
from dataclasses import dataclass

UNIT=1_000_000
MAX_QUANTITY=2_147_483_647
TICK=5000


@dataclass(frozen=True)
class ApiBudget:
    reads_per_minute:int=100
    writes_per_minute:int=30
    read_completion_gap_ms:int=0
    write_completion_gap_ms:int=0
    source_sha256:str='a7d5b76ce9c79afdb8ad327bd606d0b8d1287904dd690dd1a0a1009c02a5092d'


@dataclass(frozen=True)
class OrderSpec:
    exchange:str
    side:str
    action:str
    quantity:int
    price:int|None=None
    expiration_ms:int|None=None

    def validate(self,now):
        if not isinstance(self.exchange,str) or not self.exchange or self.side not in ('YES','NO') or self.action not in ('BUY','SELL'):
            raise ValueError('ORDER_SCOPE_OR_SIDE')
        if type(self.quantity)is not int or not 1<=self.quantity<=MAX_QUANTITY:raise ValueError('API_QUANTITY')
        if not self.market and (type(self.price)is not int or not TICK<=self.price<=UNIT-TICK or self.price%TICK):
            raise ValueError('API_TICK')
        if self.expiration_ms is not None and (self.market or type(self.expiration_ms)is not int or self.expiration_ms<=now):
            raise ValueError('API_EXPIRATION')

    @property
    def market(self):
        return self.price is None or (self.action=='BUY' and self.price==UNIT) or (self.action=='SELL' and self.price==0)
