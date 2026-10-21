//! Live account feedback is derived only from scoped, owned official receipts.
pub mod feedback;
pub mod authority;
pub mod daily_windows;
pub mod package_admission;
pub mod package_authority;
pub mod package_feedback;
pub mod package_not_sent_evidence;
pub mod package_never_attempted;
pub mod package_worker;
pub mod package_settings;
pub mod package_preflight;
pub mod package_dispatch;
pub mod dispatch;
pub mod expired_exposure;

#[cfg(test)]mod expired_allocation_boundary_test{
 #[test]fn restored_expired_exposure_is_not_a_network_abort_or_serialized_permission(){
  let(root,log)=crate::expired_exposure::tests::resolved_fixture();
  let rows=log.exposure_records().clone();assert!(log.exposure_resolved(1));assert!(!log.proved_no_network(1)&&!log.never_attempted(1));
  // Exact retained journal membership and process-local proof hydration are
  // separately required by the allocation boundary.
  assert!(crate::collateral_feedback::test_exact_expired_resolution(&log,&rows,1));
  assert!(!crate::collateral_feedback::test_exact_expired_resolution(&log,&Default::default(),1));
  let mut changed=rows.clone();changed.get_mut(&1).unwrap()["proofSha256"]=serde_json::json!("0".repeat(64));assert!(!crate::collateral_feedback::test_exact_expired_resolution(&log,&changed,1));
  drop(log);let replay=crate::dispatch::DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();assert!(!crate::collateral_feedback::test_exact_expired_resolution(&replay,&rows,1));drop(replay);std::fs::remove_dir_all(root).unwrap();
 }
}
pub mod breadth;
pub mod read_recovery;
pub mod universe;
pub mod throughput_discovery;
pub mod discovery_flow;
pub mod handoff;
pub mod inventory;
pub mod relationship_census;
pub mod relationship_strategy;
pub mod reconciliation_plan;
pub mod reconciliation;
pub mod dispatch_window;

pub mod scheduling;
pub mod exit_refresh;
pub mod exit_refresh_snapshot;
pub mod stream_recovery;
pub mod collateral_reconciliation;
pub mod stream_connect_retry;
pub mod stream_token_retry;

pub mod package_first_call;

pub mod package_portfolio;
pub mod execution_edge;
pub mod package_portfolio_feedback;
pub mod collateral_feedback;
pub mod package_portfolio_worker;
pub mod quote_priority;
pub mod package_allocation;
pub mod maker_policy;
pub mod fundamental_policy;

pub mod graph_discovery;
pub mod alpha_proposal;
pub mod execution_distribution;
pub mod source_innovation;
pub mod election_nowcast;
pub mod decision_trace;
pub mod value_surface;
pub mod election_curve;
pub mod execution_router;
pub mod semantic_states;
pub mod probabilistic_portfolio;
pub mod paper5_runtime;
pub mod probabilistic_portfolio_worker;
pub mod owned_age_reads;
pub mod stale_entry_drafts;
pub mod bulk_price_screen;

pub mod capital_allocation;
pub mod ordinary_side_edge;
pub mod prize_utility;
pub mod serviced_read;

pub mod startup_ack;

pub mod account_realtime;

pub mod fundamental_reload;

pub mod order_snapshot;
pub mod ledger_snapshot;
pub mod tartarus_snapshot;

pub mod rank_campaign;
