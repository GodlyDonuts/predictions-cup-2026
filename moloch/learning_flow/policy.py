from moloch.learning_v2.market_memory_policy import MarketMemoryPolicy
from .episodes import CONTRACT_FEATURES,GLOBAL_FEATURES


class FlowPolicy(MarketMemoryPolicy):
    architecture='tartarus-observed-interval-market-and-global-gru-trade-flow-v5'

    def __init__(self,market_slots,hidden_size=1024,market_hidden_size=64):
        super().__init__(market_slots,CONTRACT_FEATURES,GLOBAL_FEATURES,hidden_size,market_hidden_size)
