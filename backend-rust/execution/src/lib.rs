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

pub mod basket_risk;
pub mod candidate_markouts;
pub mod candidate_markouts_audit;
pub mod controller;
pub mod core;
pub mod decision;
pub mod decision_audit;
pub mod entry_gates;
pub mod entry_markouts;
pub mod entry_path_quality;
pub mod entry_path_quality_audit;
pub mod frozen;
pub mod gates;
pub mod median;
pub mod median_controller;
pub mod owned_risk_timing_audit;
pub mod production;
pub mod receipts;
pub mod reference_quality_audit;
pub mod settlement;
pub mod median_controller_v2;
pub mod median_controller_v2_clock;
pub mod median_reference;
pub mod median_preflight;
pub mod median_protected_candidate_plan;
pub mod transport;
