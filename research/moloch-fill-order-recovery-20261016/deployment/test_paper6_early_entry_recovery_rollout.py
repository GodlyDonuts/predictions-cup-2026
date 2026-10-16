import ast,copy,json,pathlib,unittest
from unittest.mock import patch
import paper6_early_entry_recovery_rollout as op
class RecoveryGuards(unittest.TestCase):
 def test_review_refresh_rejects_unrelated_reviewer(self):
  review=copy.deepcopy(op.OLD_NC_REVIEW);review['reviewerId']='unrelated-agent'
  with self.assertRaisesRegex(RuntimeError,'EXACT_CURRENT_REVIEWER_REQUIRED'):op.review_delta(op.OLD_NC_REVIEW,review,op.OLD_AUTHORITY)
 def test_pending_build_refuses_before_any_operation(self):
  for mode in ['stage','promote']:
   with patch.object(op.subprocess,'run',side_effect=AssertionError('NO_OPERATION')):
    with self.assertRaisesRegex(RuntimeError,'SOURCE_NOT_FROZEN'):op.main(['draft',mode,'/not-used','a'*64])
 def test_stop_mode_and_controlled_stop_proof_are_absent(self):
  with self.assertRaisesRegex(RuntimeError,'USAGE_STAGE_PROMOTE_ONLY'):op.main(['draft','stop','/not-used','a'*64])
  self.assertFalse(hasattr(op,'checked_witness'))
  source=pathlib.Path(op.__file__).read_text();self.assertNotIn('controlled-worker-stop-only-not-human-stop',source);self.assertNotIn('stop-request.json',source)
 def test_failure_exact_originals_and_positive_report(self):
  stop=b'original failure STOP';status=b'original stopped status';receipt=b'original ambiguous receipt';ss=op.hash_bytes(stop);ts=op.hash_bytes(status);rs=op.hash_bytes(receipt)
  report={'reason':'validated-same-timestamp-sell-fill-recovery','oldPid':op.OLD_PID,'oldSourceCommit':op.PRIOR_SOURCE,'oldBinarySha256':op.PRIOR_BINARY,'failureStopSha256':ss,'failureStatusSha256':ts,'ambiguousReceiptSha256':rs,'knownOriginalSellFillsVerified':True,'buyAmbiguityStillRejected':True,'nativeSourcesChanged':False,'sdkSourcesChanged':False};raw=json.dumps(report).encode()
  with patch.multiple(op,FAILURE_STOP_SHA=ss,FAILURE_STATUS_SHA=ts,AMBIGUOUS_RECEIPT_SHA=rs,RECOVERY_REVIEW_SHA=op.hash_bytes(raw)):
   op.checked_failure(stop,status,receipt,raw)
   for inputs in [(stop+b' ',status,receipt,raw),(stop,status+b' ',receipt,raw),(stop,status,receipt+b' ',raw)]:
    with self.assertRaisesRegex(RuntimeError,'EXACT_ORIGINAL_FAILURE_PINS'):op.checked_failure(*inputs)
   bad=dict(report,knownOriginalSellFillsVerified=False);badraw=json.dumps(bad).encode()
   with patch.object(op,'RECOVERY_REVIEW_SHA',op.hash_bytes(badraw)),self.assertRaisesRegex(RuntimeError,'RECOVERY_REVIEW_FACTS_REQUIRED'):op.checked_failure(stop,status,receipt,badraw)
 def test_controlled_or_other_failure_witness_cannot_clear_stop(self):
  b={'manifestSha256':'a'*64}
  with patch.multiple(op,FAILURE_STOP_SHA='b'*64,FAILURE_STATUS_SHA='c'*64,AMBIGUOUS_RECEIPT_SHA='d'*64,RECOVERY_REVIEW_SHA='e'*64):
   w={'operator':op.OPERATOR,'executionOwnerReviewed':True,'oldPid':op.OLD_PID,'manifestSha256':b['manifestSha256'],'failureStopSha256':op.FAILURE_STOP_SHA,'failureStatusSha256':op.FAILURE_STATUS_SHA,'ambiguousReceiptSha256':op.AMBIGUOUS_RECEIPT_SHA,'recoveryReviewSha256':op.RECOVERY_REVIEW_SHA,'reason':'validated-same-timestamp-sell-fill-recovery'};op.failure_witness(w,b)
   for key,value in [('reason','controlled-worker-stop-only-not-human-stop'),('oldPid',518852),('failureStopSha256','f'*64),('executionOwnerReviewed',False)]:
    bad=dict(w);bad[key]=value
    with self.assertRaisesRegex(RuntimeError,'INDEPENDENT_FAILURE_RECOVERY_WITNESS_REQUIRED'):op.failure_witness(bad,b)
 def test_exact_three_source_delta_rejects_native_or_sdk_change(self):
  old={f'fixture/{i}':'a'*64 for i in range(246)};old.update({k:'a'*64 for k in op.ALLOWED_PATHS});old['moloch/native/src/engine.rs']='a'*64;old['backend-rust/transport-service/src/lib.rs']='a'*64;self.assertEqual(len(old),251);new=dict(old);changes={k:'b'*64 for k in op.ALLOWED_PATHS};new.update(changes)
  with patch.object(op,'CHANGES',changes):
   op.checked_sources(old,new)
   for key in ['moloch/native/src/engine.rs','backend-rust/transport-service/src/lib.rs']:
    bad=dict(new);bad[key]='c'*64
    with self.assertRaisesRegex(RuntimeError,'UNEXPECTED_SOURCE_CHANGE'):op.checked_sources(old,bad)
 def test_stage_receipt_schema_has_no_legacy_failed_worker_keys(self):
  tree=ast.parse(pathlib.Path(op.__file__).read_text());stage=next(n.value.args[1] for n in ast.walk(tree) if isinstance(n,ast.Expr) and isinstance(n.value,ast.Call) and isinstance(n.value.func,ast.Name) and n.value.func.id=='save' and len(n.value.args)>1 and isinstance(n.value.args[0],ast.Name) and n.value.args[0].id=='stagefile');keys={k.value for k in stage.keys};reads={n.slice.value for n in ast.walk(tree) if isinstance(n,ast.Subscript) and isinstance(n.value,ast.Name) and n.value.id=='s' and isinstance(n.slice,ast.Constant)};self.assertLessEqual(reads,keys);self.assertNotIn('failedWorkerLog',reads)
 def test_no_checked_build_or_nc_or_source_pin_fabricated(self):
  for key in ['SOURCE','BINARY_SHA','SOURCE_MANIFEST_SHA','VALIDATION_SHA','EXPECTED_CHECKS','TEST_LOGS','CHANGES','NEW_SETTINGS_CANONICAL_SHA','NEW_NC_REVIEW_RAW_SHA','NEW_NC_REVIEW_RUST_SHA','NEW_OR_REVIEW_RAW_SHA','NEW_OR_REVIEW_RUST_SHA','NEW_AUTHORITY_RAW_SHA','FUNDAMENTAL_VALIDATION_SHA','FAILURE_STOP_SHA','FAILURE_STATUS_SHA','AMBIGUOUS_RECEIPT_SHA','RECOVERY_REVIEW_SHA']:self.assertIsNone(getattr(op,key),key)
 def test_two_review_settings_preserve_de_ma_and_append_both_trust_hashes(self):
  review=copy.deepcopy(op.OLD_NC_REVIEW);review['reviewerId']='Rhaegal-Viserion-paper6-20261016';review['maximumPrincipal']='2000.000000';review['reviewedAtMs']+=18000000;review['expiresAtMs']=review['reviewedAtMs']+3600000
  for row in review['sources']:
   if row['kind']!='official_contract_node':row['observedAtMs']=review['reviewedAtMs']
  authority=copy.deepcopy(op.OLD_AUTHORITY);authority['trustedReviewSha256']+=['a'*64,'b'*64]
  settings=copy.deepcopy(op.OLD_SETTINGS);settings['authorityPath']='/new/authority';settings['authoritySha256']='c'*64;settings['reviews'][2]['path']='/new/nc';settings['reviews'][2]['fileSha256']='d'*64;settings['reviews'].append({'path':'/new/or','fileSha256':'e'*64,'sourceFiles':[]})
  pins={'NEW_SETTINGS_CANONICAL_SHA':op.canonical_settings_sha(settings),'NEW_NC_REVIEW_RAW_SHA':'d'*64,'NEW_NC_REVIEW_RUST_SHA':'a'*64,'NEW_OR_REVIEW_RAW_SHA':'e'*64,'NEW_OR_REVIEW_RUST_SHA':'b'*64,'NEW_AUTHORITY_RAW_SHA':'c'*64}
  with patch.multiple(op,**pins):op.settings_delta(op.OLD_SETTINGS,settings);op.review_delta(op.OLD_NC_REVIEW,review,authority)
  for which in [0,1]:
   bad=copy.deepcopy(settings);bad['reviews'][which]['path']='/different'
   with patch.multiple(op,**dict(pins,NEW_SETTINGS_CANONICAL_SHA=op.canonical_settings_sha(bad))),self.assertRaisesRegex(RuntimeError,'DE_MA_SETTINGS_MUST_STAY_IDENTICAL'):op.settings_delta(op.OLD_SETTINGS,bad)
  bad=copy.deepcopy(authority);bad['trustedReviewSha256'].pop()
  with patch.multiple(op,**pins),self.assertRaisesRegex(RuntimeError,'APPEND_ONLY_REVIEW_AUTHORITY_REQUIRED'):op.review_delta(op.OLD_NC_REVIEW,review,bad)
if __name__=='__main__':unittest.main()
