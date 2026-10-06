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
pub mod basket_risk;
pub mod candidate_markouts;
pub mod candidate_markouts_audit;
pub mod canonical_fifo;
pub mod canonical_no_capacity;
pub mod canonical_order_journal;
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
pub mod gates;
pub mod ledger;
pub mod median;
pub mod median_controller;
pub mod median_controller_v2;
pub mod median_controller_v2_clock;
pub mod median_preflight;
pub mod median_protected_candidate_plan;
pub mod median_reference;
pub mod no_neutral_all;
pub mod no_order_economics;
pub mod no_rest_reconciliation;
pub mod owned_risk_timing_audit;
pub mod paired_dry_run;
pub mod paired_portfolio;
pub mod performance;
pub mod performance_watch;
pub mod production;
pub mod production_activation;
pub mod production_entry_state;
pub mod production_reconciliation;
pub mod receipt_production_bank;
pub mod receipts;
pub mod reference_quality_audit;
pub mod settlement;
pub mod settlement_bank_transition;
pub mod settlement_controller;
pub mod side_accounting;
pub mod transport;
