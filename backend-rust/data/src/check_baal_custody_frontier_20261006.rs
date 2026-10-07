// data/check-baal-custody-frontier-20261006.mjs — offline check that the
// complete baal custody frontier still requires a fresh account before any
// successor can continue. run() returns the JSON the script prints.
use serde_json::{json, Value as V};

use crate::baal_custody_frontier_v1::{
    assert_baal_custody_frontier_can_continue, read_baal_custody_frontier_v1,
};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn set(v: &mut V, k: &str, x: V) {
    if let Some(m) = v.as_object_mut() {
        m.insert(k.to_string(), x);
    }
}

// assert.equal(actual, expected) — strict-equal semantics for these scalars.
fn assert_equal(actual: &V, expected: &V) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "AssertionError [ERR_ASSERTION]: {} !== {}",
            baal_service_runtime::json_text::js_stringify(actual),
            baal_service_runtime::json_text::js_stringify(expected)
        ))
    }
}

// assert.throws(fn, /code/) — the thrown error's message must match the code.
fn assert_throws(result: Result<V, String>, code: &str) -> Result<(), String> {
    match result {
        Err(e) if e.contains(code) => Ok(()),
        Err(e) => Err(format!(
            "AssertionError [ERR_ASSERTION]: expected error matching /{code}/, got {e}"
        )),
        Ok(_) => Err(format!(
            "AssertionError [ERR_ASSERTION]: Missing expected exception matching /{code}/"
        )),
    }
}

pub fn run() -> Result<V, String> {
    let runtime_dir = crate::publication_pack::runtime_dir();
    let workspace = std::env::current_dir().map_err(|e| e.to_string())?;
    let f = read_baal_custody_frontier_v1(&json!({
        "runtimeDir": runtime_dir,
        "workspace": workspace.to_string_lossy(),
    }))?;
    let c = assert_baal_custody_frontier_can_continue(&f)?;
    assert_equal(get(&c, "freshAccountStillRequired"), &V::Bool(true))?;

    let mut x = f.clone();
    set(get_mut(&mut x, "summary"), "ownedQuantity", json!(0));
    assert_throws(
        assert_baal_custody_frontier_can_continue(&x),
        "SUMMARY_ECONOMICS",
    )?;

    let mut x = f.clone();
    set(get_mut(&mut x, "currentState"), "orders", json!([]));
    assert_throws(
        assert_baal_custody_frontier_can_continue(&x),
        "EXACT_FRONTIER",
    )?;

    let mut x = f.clone();
    set(get_mut(&mut x, "risk"), "cashReserved", json!("0.000000"));
    assert_throws(
        assert_baal_custody_frontier_can_continue(&x),
        "CURRENT_FULL_RESERVATION",
    )?;

    let mut x = f.clone();
    set(&mut x, "executionAuthority", json!(true));
    assert_throws(
        assert_baal_custody_frontier_can_continue(&x),
        "EXACT_FRONTIER",
    )?;

    Ok(json!({
        "checks": 5,
        "summary": get(&f, "summary"),
        "currentStateHash": get(&f, "currentStateHash"),
        "networkRequests": 0,
        "originalJournalsChanged": false,
        "executionAuthority": false,
    }))
}

fn get_mut<'a>(v: &'a mut V, k: &str) -> &'a mut V {
    if v.get(k).is_none() {
        v.as_object_mut().map(|m| m.insert(k.to_string(), V::Null));
    }
    v.get_mut(k).unwrap()
}
