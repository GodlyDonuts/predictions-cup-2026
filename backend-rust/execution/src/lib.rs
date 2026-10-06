// baal-execution: port of execution/*.mjs — the order path's offline core,
// transport adapter, controllers, median/settlement strategies and audits.
pub const EXECUTION_HASH: &str = env!("BAAL_EXECUTION_HASH");
pub const TOURNAMENT_ID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
pub const TOURNAMENT_SLUG: &str = "midterm-elections";

pub mod controller;
pub mod core;
pub mod decision;
pub mod frozen;
pub mod gates;
pub mod median;
pub mod median_controller;
pub mod production;
pub mod receipts;
pub mod settlement;
pub mod transport;

// JS String(value) for an order id / loose value used in endpoint paths and
// comparisons — integers print without a decimal point.
pub fn js_key(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(_) => baal_service_runtime::json_text::js_stringify(v),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".into(),
        _ => baal_service_runtime::json_text::js_stringify(v),
    }
}
// JS String(x) on a JSON value for decimalMicros(String(x)).
pub fn json_number(v: &serde_json::Value) -> String {
    js_key(v)
}
