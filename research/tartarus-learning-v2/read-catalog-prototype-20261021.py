"""Ordered API keys with indexed lookup; never an admission or observation cache.

Only immutable built-in keys use the index. Other values retain list behavior.
Every mutation invalidates it, including partially failed iterable operations.
"""

def immutable(value):
    return type(value) in (str, bytes, int, bool, float, type(None)) or (
        type(value) is tuple and all(immutable(item) for item in value))


class ReadCatalog(list):
    def __init__(self, values=()):
        super().__init__(values)
        self._positions = None
        self._mutating = 0

    def _index(self):
        if self._positions is None:
            if not all(immutable(item) for item in self):
                return None
            positions = {}
            for index, item in enumerate(self):
                positions.setdefault(item, index)
            self._positions = positions
        return self._positions

    def __contains__(self, value):
        positions = self._index() if not self._mutating and immutable(value) else None
        return value in positions if positions is not None else super().__contains__(value)

    def index(self, value, *bounds):
        positions = self._index() if not bounds and not self._mutating and immutable(value) else None
        if positions is None:
            return super().index(value, *bounds)
        try:
            return positions[value]
        except KeyError:
            raise ValueError(f'{value!r} is not in list') from None

    def _change(self, method, *args, **kwargs):
        self._positions = None
        self._mutating += 1
        try:
            return method(self, *args, **kwargs)
        finally:
            self._mutating -= 1
            self._positions = None

    def append(self, value): return self._change(list.append, value)
    def extend(self, values): return self._change(list.extend, values)
    def insert(self, index, value): return self._change(list.insert, index, value)
    def pop(self, *args): return self._change(list.pop, *args)
    def remove(self, value): return self._change(list.remove, value)
    def clear(self): return self._change(list.clear)
    def reverse(self): return self._change(list.reverse)
    def sort(self, *args, **kwargs): return self._change(list.sort, *args, **kwargs)
    def __setitem__(self, index, value): return self._change(list.__setitem__, index, value)
    def __delitem__(self, index): return self._change(list.__delitem__, index)
    def __iadd__(self, values): return self._change(list.__iadd__, values)
    def __imul__(self, count): return self._change(list.__imul__, count)
