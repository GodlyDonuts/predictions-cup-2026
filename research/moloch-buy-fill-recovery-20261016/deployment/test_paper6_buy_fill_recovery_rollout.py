import ast, copy, json, pathlib, unittest
from unittest.mock import patch
import paper6_buy_fill_recovery_rollout as op

class RecoveryGuards(unittest.TestCase):

    def test_pending_build_refuses_before_any_operation(self):
        for mode in ['stage', 'promote']:
            with patch.object(op.subprocess, 'run', side_effect=AssertionError('NO_OPERATION')):
                with self.assertRaisesRegex(RuntimeError, 'SOURCE_NOT_FROZEN'):
                    op.main(['draft', mode, '/not-used', 'a' * 64])

    def test_stop_mode_and_controlled_stop_proof_are_absent(self):
        with self.assertRaisesRegex(RuntimeError, 'USAGE_STAGE_PROMOTE_ONLY'):
            op.main(['draft', 'stop', '/not-used', 'a' * 64])
        self.assertFalse(hasattr(op, 'checked_witness'))
        source = pathlib.Path(op.__file__).read_text()
        self.assertNotIn('controlled-worker-stop-only-not-human-stop', source)
        self.assertNotIn('stop-request.json', source)

    def test_failure_original_buy_receipts_and_explicit_fifo_review(self):
        stop, status, tape, order, ack = (b'original STOP', b'original stopped status', b'original complete tape', b'original terminal GET', b'original ACK')
        pins = dict(FAILURE_STOP_SHA=op.hash_bytes(stop), FAILURE_STATUS_SHA=op.hash_bytes(status), AMBIGUOUS_RECEIPT_SHA=op.hash_bytes(tape), ORIGINAL_ORDER_SHA=op.hash_bytes(order), ORIGINAL_ACK_SHA=op.hash_bytes(ack))
        r = {'reason': 'validated-same-timestamp-buy-fill-recovery', 'oldPid': op.OLD_PID, 'oldSourceCommit': op.PRIOR_SOURCE, 'oldBinarySha256': op.PRIOR_BINARY, 'failureStopSha256': pins['FAILURE_STOP_SHA'], 'failureStatusSha256': pins['FAILURE_STATUS_SHA'], 'ambiguousReceiptSha256': pins['AMBIGUOUS_RECEIPT_SHA'], 'originalOrderSha256': pins['ORIGINAL_ORDER_SHA'], 'originalAckSha256': pins['ORIGINAL_ACK_SHA'], 'completeTapeSemanticSha256': '5a84d4d6c62bf64f3f92cd44a845e844a83d6b4b1f7982a1bd462f9f220bda3c', 'knownOriginalBuyFillsVerified': True, 'qid': 8803, 'actualOrderId': '43530180', 'exchangeId': '977', 'marketId': '288', 'side': 'NO', 'filledQuantity': 2094, 'originalFillRowCount': 5, 'grossCost': '1949.320000', 'fillPricesAndQuantitiesPreserved': True, 'noManualFinancialRows': True, 'buyFifoConventionReviewed': True, 'buyFifoConvention': 'Synthetic fixture convention, not venue fact', 'nativeSourcesChanged': False, 'sdkSourcesChanged': False}
        raw = json.dumps(r).encode()
        with patch.multiple(op, **dict(pins, RECOVERY_REVIEW_SHA=op.hash_bytes(raw))):
            op.checked_failure(stop, status, tape, order, ack, raw)
            for i in range(5):
                args = [stop, status, tape, order, ack, raw]
                args[i] += b'changed'
                with self.assertRaisesRegex(RuntimeError, 'EXACT_ORIGINAL_FAILURE_PINS'):
                    op.checked_failure(*args)
        for key, value in [('buyFifoConventionReviewed', False), ('buyFifoConvention', ''), ('fillPricesAndQuantitiesPreserved', False), ('noManualFinancialRows', False), ('grossCost', '1949.000000'), ('qid', 8804), ('side', 'YES')]:
            bad = dict(r)
            bad[key] = value
            body = json.dumps(bad).encode()
            with patch.multiple(op, **dict(pins, RECOVERY_REVIEW_SHA=op.hash_bytes(body))), self.assertRaisesRegex(RuntimeError, 'EXPLICIT_REVIEWED_BUY_FIFO_RECOVERY_REQUIRED'):
                op.checked_failure(stop, status, tape, order, ack, body)

    def test_controlled_or_other_failure_witness_cannot_clear_stop(self):
        b = {'manifestSha256': 'a' * 64}
        with patch.multiple(op, FAILURE_STOP_SHA='b' * 64, FAILURE_STATUS_SHA='c' * 64, AMBIGUOUS_RECEIPT_SHA='d' * 64, RECOVERY_REVIEW_SHA='e' * 64, ORIGINAL_ORDER_SHA='f' * 64, ORIGINAL_ACK_SHA='1' * 64):
            w = {'operator': op.OPERATOR, 'executionOwnerReviewed': True, 'oldPid': op.OLD_PID, 'manifestSha256': b['manifestSha256'], 'failureStopSha256': op.FAILURE_STOP_SHA, 'failureStatusSha256': op.FAILURE_STATUS_SHA, 'ambiguousReceiptSha256': op.AMBIGUOUS_RECEIPT_SHA, 'recoveryReviewSha256': op.RECOVERY_REVIEW_SHA, 'originalOrderSha256': op.ORIGINAL_ORDER_SHA, 'originalAckSha256': op.ORIGINAL_ACK_SHA, 'reason': 'validated-same-timestamp-buy-fill-recovery'}
            op.failure_witness(w, b)
            for key, value in [('reason', 'controlled-worker-stop-only-not-human-stop'), ('oldPid', 518852), ('failureStopSha256', 'f' * 64), ('executionOwnerReviewed', False)]:
                bad = dict(w)
                bad[key] = value
                with self.assertRaisesRegex(RuntimeError, 'INDEPENDENT_BUY_RECOVERY_WITNESS_REQUIRED'):
                    op.failure_witness(bad, b)

    def test_exact_one_source_delta_rejects_native_or_sdk_change(self):
        old = {f'fixture/{i}': 'a' * 64 for i in range(248)}
        old.update({k: 'a' * 64 for k in op.ALLOWED_PATHS})
        old['moloch/native/src/engine.rs'] = 'a' * 64
        old['backend-rust/transport-service/src/lib.rs'] = 'a' * 64
        self.assertEqual(len(old), 251)
        new = dict(old)
        changes = {k: 'b' * 64 for k in op.ALLOWED_PATHS}
        new.update(changes)
        with patch.object(op, 'CHANGES', changes):
            op.checked_sources(old, new)
            for key in ['moloch/native/src/engine.rs', 'backend-rust/transport-service/src/lib.rs']:
                bad = dict(new)
                bad[key] = 'c' * 64
                with self.assertRaisesRegex(RuntimeError, 'UNEXPECTED_SOURCE_CHANGE'):
                    op.checked_sources(old, bad)

    def test_stage_receipt_schema_has_no_legacy_failed_worker_keys(self):
        tree = ast.parse(pathlib.Path(op.__file__).read_text())
        stage = next((n.value.args[1] for n in ast.walk(tree) if isinstance(n, ast.Expr) and isinstance(n.value, ast.Call) and isinstance(n.value.func, ast.Name) and (n.value.func.id == 'save') and (len(n.value.args) > 1) and isinstance(n.value.args[0], ast.Name) and (n.value.args[0].id == 'stagefile')))
        keys = {k.value for k in stage.keys}
        reads = {n.slice.value for n in ast.walk(tree) if isinstance(n, ast.Subscript) and isinstance(n.value, ast.Name) and (n.value.id == 's') and isinstance(n.slice, ast.Constant)}
        self.assertLessEqual(reads, keys)
        self.assertNotIn('failedWorkerLog', reads)

    def test_no_checked_build_or_source_pin_fabricated(self):
        for key in ['SOURCE', 'BINARY_SHA', 'SOURCE_MANIFEST_SHA', 'VALIDATION_SHA', 'EXPECTED_CHECKS', 'TEST_LOGS', 'CHANGES', 'PRIOR_RECEIPT', 'PRIOR_CONTROL', 'PRIOR_UNIT', 'ORIGINAL_ORDER_SHA', 'ORIGINAL_ACK_SHA', 'FAILURE_STOP_SHA', 'FAILURE_STATUS_SHA', 'AMBIGUOUS_RECEIPT_SHA', 'RECOVERY_REVIEW_SHA']:
            self.assertIsNone(getattr(op, key), key)

    def test_binary_only_control_cannot_modify_nc_or_or_or_capital(self):
        before = {'activation': {'binarySha256': op.PRIOR_BINARY, 'maximumCommitted': '95579.915000'}, 'defensiveCancel': True, 'ordinarySideEdge': 'enforce', 'fundamentalPolicy': {'reviews': [{'NC': 'exact-current'}, {'OR': 'exact-current'}]}}
        after = copy.deepcopy(before)
        after['activation']['binarySha256'] = 'a' * 64
        op.controls(before, after, 'a' * 64)
        for mutate in ['review', 'capital', 'mode']:
            bad = copy.deepcopy(after)
            if mutate == 'review':
                bad['fundamentalPolicy']['reviews'][0]['NC'] = 'changed'
            elif mutate == 'capital':
                bad['activation']['maximumCommitted'] = '200000'
            else:
                bad['ordinarySideEdge'] = 'disabled'
            with self.assertRaisesRegex(RuntimeError, 'BINARY_ONLY_CONTROL_CHANGE_REQUIRED'):
                op.controls(before, bad, 'a' * 64)
if __name__ == '__main__':
    unittest.main()
