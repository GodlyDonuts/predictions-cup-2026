#[path="support/engine_clone_fixture.rs"]mod fixture;
use fixture::*;
use serde_json::json;

#[test]fn successful_gap_changes_only_its_engine_fork(){
 let original=warm().unwrap();let expected=fingerprint(&original).unwrap();let mut fork=original.clone();
 fork.advance(&gap(0,WARM_END_MS+1,false)).unwrap();
 assert_eq!(fingerprint(&original).unwrap(),expected);
 assert_ne!(fingerprint(&fork).unwrap(),expected);
 let mut independent=warm().unwrap();let mut untouched=original.clone();
 let event=tick(WARM_END_MS+2);untouched.advance(&event).unwrap();independent.advance(&event).unwrap();
 assert_eq!(fingerprint(&untouched).unwrap(),fingerprint(&independent).unwrap());
}
#[test]fn rejected_gap_does_not_poison_shared_original_or_other_fork(){
 let original=warm().unwrap();let mut bad=original.clone();let mut sibling=original.clone();
 let before=fingerprint(&original).unwrap();assert!(bad.advance(&gap(0,WARM_END_MS+1,true)).is_err());
 assert_eq!(fingerprint(&original).unwrap(),before);assert_eq!(fingerprint(&bad).unwrap(),before);assert_eq!(fingerprint(&sibling).unwrap(),before);
 let mut independent=warm().unwrap();let event=tick(WARM_END_MS+2);
 bad.advance(&event).unwrap();sibling.advance(&event).unwrap();independent.advance(&event).unwrap();
 assert_eq!(fingerprint(&bad).unwrap(),fingerprint(&independent).unwrap());assert_eq!(fingerprint(&sibling).unwrap(),fingerprint(&independent).unwrap());
}
#[test]fn replacement_book_does_not_change_shared_old_snapshot_behavior(){
 let mut changed=warm().unwrap();let mut sibling=changed.clone();let mut independent=warm().unwrap();
 let mut event=book(0,2,WARM_END_MS+1);event["book"]["bids"][0]["quantity"]=json!(2000);
 changed.advance(&event).unwrap();
 let probe=tick(WARM_END_MS+2);sibling.advance(&probe).unwrap();independent.advance(&probe).unwrap();
 assert_eq!(fingerprint(&sibling).unwrap(),fingerprint(&independent).unwrap());
}
#[test]fn valid_gap_then_resync_matches_independent_replay(){
 let mut candidate=warm().unwrap();let mut independent=warm().unwrap();
 for event in [gap(0,WARM_END_MS+1,false),book(0,2,WARM_END_MS+2),tick(WARM_END_MS+3)]{
  candidate.advance(&event).unwrap();independent.advance(&event).unwrap();
  assert_eq!(fingerprint(&candidate).unwrap(),fingerprint(&independent).unwrap());
 }
}
