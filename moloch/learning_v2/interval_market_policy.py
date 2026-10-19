"""Explicit observed-interval feature semantics on the market/global GRU."""
from .market_memory_policy import MarketMemoryPolicy
from .interval_episodes import CONTRACT_FEATURES,GLOBAL_FEATURES


class IntervalMarketPolicy(MarketMemoryPolicy):
    architecture='tartarus-observed-interval-market-and-global-gru-v4'

    def __init__(self,market_slots,hidden_size=1024,market_hidden_size=64):
        super().__init__(market_slots,CONTRACT_FEATURES,GLOBAL_FEATURES,hidden_size,market_hidden_size)
