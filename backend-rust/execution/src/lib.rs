// baal-execution modules: execution/ port.
#![allow(clippy::too_many_arguments)]

use serde_json::Value as V;

pub const TOURNAMENT_ID: &str = baal_service_runtime::client::TOURNAMENT_ID;
pub const TOURNAMENT_SLUG: &str = "midterm-elections";

/// JS String(v) for scalar id fields (orderId comparisons etc.).
pub fn js_key(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}

/// JS String(number) used when a field arrives as a JSON number, not a string.
pub fn json_number(v: &V) -> String {
    match v {
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        other => js_key(other),
    }
}
pub mod api_tick_price;
pub mod atomic_rename;
pub mod baal_capital_sizing_v1;
pub mod baal_combined_custody_host_v6;
pub mod baal_combined_custody_host_v7;
pub mod baal_combined_custody_reader_scope_v1;
pub mod baal_custody_frontier_v1;
pub mod baal_generic_custody_seed_v1;
pub mod baal_generic_live_host_v1;
pub mod baal_generic_live_host_v2;
pub mod baal_generic_live_host_v3;
pub mod baal_generic_live_host_v4;
pub mod baal_generic_no_controller_v3;
pub mod baal_generic_no_controller_v4;
pub mod baal_generic_no_controller_v5;
pub mod baal_generic_no_evidence_bridge_v3;
pub mod baal_generic_no_evidence_bridge_v4;
pub mod baal_generic_no_evidence_bridge_v5;
pub mod baal_generic_no_evidence_bridge_v6;
pub mod baal_generic_prior_custody_v1;
pub mod baal_generic_quote_context_v1;
pub mod baal_generic_shared_budget_v3;
pub mod baal_generic_shared_budget_v4;
pub mod baal_generic_shared_budget_v5;
pub mod baal_neutral_owned_counterpart_pilot_v2;
pub mod baal_owned_all_read_host_v4;
pub mod baal_owned_exit_priority_v1;
pub mod baal_owned_hedge_shared_budget_v2;
pub mod baal_paired_liquidity_sweep_v1;
pub mod baal_profitable_exit_depth_sweep_v1;
pub mod baal_recovered_custody_seed_v2;
pub mod baal_scaled_controller_legacy_replay_v1;
pub mod baal_scaled_entry_signal_v1;
pub mod baal_scaled_entry_signal_v2;
pub mod baal_scaled_legacy_risk_v1;
pub mod baal_scaled_live_host_v1;
pub mod baal_scaled_live_host_v2;
pub mod baal_scaled_live_host_v3;
pub mod baal_scaled_live_host_v5;
pub mod baal_scaled_no_controller_v1;
pub mod baal_scaled_no_evidence_bridge_v1;
pub mod baal_scaled_owned_exit_read_hint_v1;
pub mod baal_scaled_owned_no_hedge_proposal_v1;
pub mod baal_scaled_profitable_exit_signal_v2;
pub mod baal_scaled_profitable_fifo_prefix_v1;
pub mod baal_scaled_profitable_fifo_prefix_v3;
pub mod baal_scaled_shared_budget_v1;
pub mod baal_scaled_shared_budget_v2;
pub mod baal_scaled_shared_budget_v3;
pub mod basket_risk;
pub mod blocked_admission_audit_v1;
pub mod blocked_admission_read_collector_v1;
pub mod blocked_admission_reconciliation_v1;
pub mod blocked_admission_recovery_host_v1;
pub mod blocked_network_proof_v1;
pub mod breadth;
pub mod candidate_markouts;
pub mod candidate_markouts_audit;
pub mod canonical_fifo;
pub mod canonical_no_capacity;
pub mod canonical_order_journal;
pub mod cohort_profit_locked_no_exit_v1;
pub mod controller;
pub mod core;
pub mod decision;
pub mod decision_audit;
pub mod dual_no_review_v1;
pub mod entry_gates;
pub mod entry_markouts;
pub mod entry_path_quality;
pub mod entry_path_quality_audit;
pub mod exact_money_receipts;
pub mod frozen;
pub mod frozen_paper_graph;
pub mod gates;
pub mod generic_read_packet_retry_v1;
pub mod ledger;
pub mod median;
pub mod median_controller;
pub mod median_controller_v2;
pub mod median_controller_v2_clock;
pub mod median_preflight;
pub mod median_protected_candidate_plan;
pub mod median_reference;
pub mod model;
pub mod neutral_sell_recovery_host_v1;
pub mod no_liquidation_depth_value_v1;
pub mod no_neutral_all;
pub mod no_neutral_sell_all_v1;
pub mod no_order_economics;
pub mod no_rest_multishare_reconciliation_v1;
pub mod no_rest_reconciliation;
pub mod no_rest_reconciliation_v2;
pub mod no_rest_sell_reconciliation_v1;
pub mod no_rest_sell_reconciliation_v2;
pub mod owned_no_exit_controller_v1;
pub mod owned_no_exit_controller_v2;
pub mod owned_no_exit_evidence_bridge_v1;
pub mod owned_no_fresh_none_hedge_evidence_bridge_v1;
pub mod owned_no_fresh_none_hedge_evidence_bridge_v2;
pub mod owned_no_fresh_none_hedge_evidence_bridge_v3;
pub mod owned_no_fresh_none_hedge_evidence_bridge_v4;
pub mod owned_no_hedge_plan_v1;
pub mod owned_no_profit_host_v1;
pub mod owned_risk_timing_audit;
pub mod paired_dry_run;
pub mod paired_live_host;
pub mod paired_live_host_v2;
pub mod paired_neutral_recovery_host_v1;
pub mod paired_no_exit_plan_v1;
pub mod paired_no_liquidation;
pub mod paired_owned_exit_retirement_v1;
pub mod paired_owned_exit_retirement_v2;
pub mod paired_portfolio;
pub mod paired_rest_controller;
pub mod paired_rest_controller_v3;
pub mod paired_rest_cycle;
pub mod paired_rest_evidence_bridge;
pub mod paired_shared_funding;
pub mod party_winner_no_screen_v1;
pub mod performance;
pub mod performance_watch;
pub mod prepare_baal_combined_custody_v6;
pub mod prepare_baal_combined_read_v7;
pub mod prepare_baal_generic_live_v1;
pub mod prepare_baal_generic_live_v2;
pub mod production;
pub mod production_activation;
pub mod production_entry_state;
pub mod production_reconciliation;
pub mod proven_network_guard_v1;
pub mod read_only_receipt_callback_v1;
pub mod read_timing;
pub mod receipt_production_bank;
pub mod receipts;
pub mod reference_quality_audit;
pub mod scoped_rest_receipts;
pub mod scoped_rest_receipts_v2;
pub mod scoped_rest_receipts_v3;
pub mod scoped_rest_receipts_v5;
pub mod scoped_rest_receipts_v6;
pub mod settlement;
pub mod settlement_bank_transition;
pub mod settlement_controller;
pub mod side_accounting;
pub mod single_owned_no_exit_signal_v1;
pub mod transport;
