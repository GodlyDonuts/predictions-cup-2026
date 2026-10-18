import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from .verify_learning_transport import source_hash


class OriginalSourceHash(unittest.TestCase):
    def test_exact_decimal_tokens_do_not_need_python_float_round_trip(self):
        unsigned=b'{"event":{"p":0.000000123456789123456789,"q":10000},"previous":"","sequence":1}'
        h=hashlib.sha256(unsigned).hexdigest()
        raw=unsigned.replace(b',"previous"',(',"hash":"'+h+'","previous"').encode())+b'\n'
        source_hash(raw,json.loads(raw))
        # Even if an outer transport envelope's digest is recomputed, altered
        # original prices must fail the independent original-producer hash.
        altered=raw.replace(b'"q":10000',b'"q":20000')
        with self.assertRaisesRegex(ValueError,'PRODUCER_HASH_MISMATCH'):
            source_hash(altered,json.loads(altered))


if __name__=='__main__':unittest.main()
