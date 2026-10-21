"""Apply the narrow worker-only R66 expiry fix without importing other edits."""
from pathlib import Path
import sys


def patched(text):
    changes = [
        ('fn expected_ordinary_final_guard(code:&str)->bool{',
         '/// Used only after an SDK proof of no HTTP and retirement of the original key.\n'
         'fn expired_fundamental_final_guard(code:&str,end:u64,original_expiry:Option<u64>)->bool{\n'
         ' code=="FUNDAMENTAL_PHYSICAL_TOKEN_EXPIRED_OR_REGRESSED"&&original_expiry.is_some_and(|expiry|end>=expiry)\n'
         '}\nfn expected_ordinary_final_guard(code:&str)->bool{'),
        (' let prepared=if let Some(r)=&rank{authority.prepare_with_rank',
         ' let fundamental_expires_at_ms=fundamental_decision.as_ref().map(|decision|decision.expires_at_ms());\n'
         ' let prepared=if let Some(r)=&rank{authority.prepare_with_rank'),
        ('return if expected_ordinary_final_guard(&error.code)||rank_expired{Ok(())}',
         'return if expected_ordinary_final_guard(&error.code)||rank_expired||expired_fundamental_final_guard(&error.code,end,fundamental_expires_at_ms){Ok(())}'),
        ('mod local_final_guard_recovery_tests{\n use super::*;',
         'mod local_final_guard_recovery_tests{\n use super::*;\n'
         ' #[test]fn original_fundamental_expiry_defers_only_a_forward_clock_miss(){\n'
         '  let code="FUNDAMENTAL_PHYSICAL_TOKEN_EXPIRED_OR_REGRESSED";\n'
         '  assert!(expired_fundamental_final_guard(code,1000,Some(1000)));\n'
         '  assert!(expired_fundamental_final_guard(code,1001,Some(1000)));\n'
         '  assert!(!expired_fundamental_final_guard(code,999,Some(1000)));\n'
         '  assert!(!expired_fundamental_final_guard(code,1001,None));\n'
         '  for other in ["FUNDAMENTAL_PHYSICAL_TAIL_CHANGED","FUNDAMENTAL_PHYSICAL_FROZEN_REQUEST_CHANGED","LIVE_BINARY_CHANGED","STOPPED_OR_OWNER_LOST"]{assert!(!expired_fundamental_final_guard(other,1001,Some(1000)));}\n'
         '  assert!(!expected_ordinary_final_guard(code));\n'
         ' }')]
    for old,new in changes:
        if text.count(old)!=1:raise ValueError('EXACT_ORIGINAL_WORKER_FRAGMENT_REQUIRED')
        text=text.replace(old,new)
    return text


if __name__=='__main__':
    path=Path(sys.argv[1]);original=path.read_text(encoding='utf-8')
    path.write_text(patched(original),encoding='utf-8')
