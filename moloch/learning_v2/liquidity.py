"""Conservative shared YES/NO displayed-depth counterfactual.

This is an execution assumption, not a queue/fill calibration. Counterfactual
depletion persists at each original YES price: unchanged snapshots and version
increments cannot manufacture replenishment. Passive queue fills are absent.
"""
from dataclasses import dataclass
from .account import UNIT, natural


@dataclass(frozen=True)
class Book:
    exchange: str
    available_at_ms: int
    source_at_ms: int | None
    bids: tuple[tuple[int, int], ...]
    asks: tuple[tuple[int, int], ...]
    original_sha256: str

    def __post_init__(self):
        natural(self.available_at_ms, 'BOOK_CLOCK')
        if self.source_at_ms is not None:
            natural(self.source_at_ms, 'SOURCE_CLOCK')
            if self.source_at_ms > self.available_at_ms:
                raise ValueError('SOURCE_CLOCK_AHEAD')
        if len(self.original_sha256) != 64 or any(c not in '0123456789abcdef' for c in self.original_sha256):
            raise ValueError('ORIGINAL_HASH')
        for name, levels in [('bids', self.bids), ('asks', self.asks)]:
            for price, quantity in levels:
                if type(price) is not int or not 0 < price < UNIT or type(quantity) is not int or quantity <= 0:
                    raise ValueError('BOOK_LEVEL')
            prices = [p for p, _ in levels]
            if len(set(prices)) != len(prices) or prices != sorted(prices, reverse=name == 'bids'):
                raise ValueError('BOOK_SORT_OR_DUPLICATE')
        if self.bids and self.asks and self.bids[0][0] >= self.asks[0][0]:
            raise ValueError('CROSSED_OR_LOCKED_BOOK')


class SharedLiquidity:
    def __init__(self, book: Book):
        self.book = book
        self.depleted: dict[tuple[str, int], int] = {}
        self.invalidated = False

    def invalidate(self):
        # A stream gap removes tradability, never the counterfactual debt.
        self.invalidated = True

    def update(self, book: Book):
        if book.exchange != self.book.exchange or book.available_at_ms < self.book.available_at_ms:
            raise ValueError('BOOK_SCOPE_OR_INFORMATION_CLOCK')
        # A new timestamp/hash is evidence of a new observation, not evidence
        # that our counterfactual volume has been replenished.
        self.book = book
        self.invalidated = False

    def visible(self, now_ms: int, maximum_age_ms: int) -> bool:
        age = now_ms - self.book.available_at_ms
        if self.invalidated or not 0 <= age <= maximum_age_ms:
            return False
        return self.book.source_at_ms is None or 0 <= now_ms - self.book.source_at_ms <= maximum_age_ms

    def cross(self, side: str, action: str, limit_price: int, quantity: int,
              now_ms: int, maximum_age_ms: int) -> tuple[tuple[int, int], ...]:
        """Call at modeled arrival using the last causally available book.

        Returns (executed side price, quantity). No unseen depth, stale book,
        passive fill, or unlimited top-price quantity is substituted.
        """
        natural(quantity, 'QUANTITY')
        if side not in ['YES', 'NO'] or action not in ['BUY', 'SELL']:
            raise ValueError('ACTION_OR_SIDE')
        if type(limit_price) is not int or not 0 < limit_price < UNIT:
            raise ValueError('LIMIT_PRICE')
        if not self.visible(now_ms, maximum_age_ms):
            return ()
        resource = 'asks' if (side, action) in [('YES', 'BUY'), ('NO', 'SELL')] else 'bids'
        result = []
        left = quantity
        for yes_price, advertised in getattr(self.book, resource):
            price = yes_price if side == 'YES' else UNIT - yes_price
            if (action == 'BUY' and price > limit_price) or (action == 'SELL' and price < limit_price):
                break
            key = (resource, yes_price)
            take = min(left, max(0, advertised - self.depleted.get(key, 0)))
            if take:
                result.append((price, take))
                self.depleted[key] = self.depleted.get(key, 0) + take
                left -= take
            if not left:
                break
        return tuple(result)
