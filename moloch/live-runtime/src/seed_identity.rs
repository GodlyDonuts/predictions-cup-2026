//! Validate retained seed identity without rewriting legacy tournament receipts.
//! The caller verifies the original receipt digest before reaching this check.
use baal_transport_service::{Receipt, Scope, decimal_units, relationships::uuid};
use serde_json::Value;

pub fn validate(original: &Receipt, current: &Receipt, seed: &Value,
                scope: &Scope, manifest: &Value, owner: Option<&Value>)
                -> Result<(), &'static str> {
    let fail = "TARTARUS_ORIGINAL_PROFILE_CHANGED";
    let profile = current.data["id"].as_str().filter(|s| uuid(s)).ok_or(fail)?;
    if original.status != 200 || current.status != 200
        || current.endpoint != format!("/account?tournamentId={}", scope.tournament_id)
        || seed["observedAt"].as_str().filter(|s| !s.is_empty()) != Some(original.received_at.as_str()) {
        return Err(fail);
    }
    if let Some(record) = owner {
        if record["bankScope"]["profileId"].as_str() != Some(profile) { return Err(fail); }
    }
    if original.endpoint == current.endpoint {
        return if original.data["id"] == current.data["id"] { Ok(()) } else { Err(fail) };
    }
    // Legacy seeds recorded a tournament balance, whose UUID is not a profile.
    // Require a separately durable owner record to bind the actual user identity.
    let record = owner.ok_or(fail)?;
    let bank = &record["bankScope"];
    let run = seed["runId"].as_str().filter(|s| !s.is_empty()).ok_or(fail)?;
    let manifest_hash = manifest.as_str().filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())).ok_or(fail)?;
    if original.endpoint != format!("/tournaments/{}", scope.tournament_slug)
        || !uuid(&scope.tournament_id)
        || original.data["id"].as_str() != Some(scope.tournament_id.as_str())
        || original.data["slug"].as_str() != Some(scope.tournament_slug.as_str())
        || bank["runId"].as_str() != Some(run)
        || bank["bankManifestHash"].as_str() != Some(manifest_hash)
        || bank["tournamentId"].as_str() != Some(scope.tournament_id.as_str())
        || bank["tournamentSlug"].as_str() != Some(scope.tournament_slug.as_str()) {
        return Err(fail);
    }
    let original_cash = &original.data["myBalance"];
    if !original_cash.is_number() { return Err(fail); }
    let cash = decimal_units(&original_cash.to_string()).map_err(|_| fail)?;
    let seeded = decimal_units(seed["availableCash"].as_str().ok_or(fail)?).map_err(|_| fail)?;
    if cash < 0 || cash != seeded { return Err(fail); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const T: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
    const P: &str = "11111111-2222-4333-8444-555555555555";
    fn fixture() -> (Receipt, Receipt, Value, Scope, Value, Value) {
        let original = Receipt { endpoint: "/tournaments/cup".into(), request_started_at: "start".into(), received_at: "seed-time".into(), status: 200, data: json!({"id":T,"slug":"cup","myBalance":70850.43}) };
        let current = Receipt { endpoint: format!("/account?tournamentId={T}"), received_at: "now".into(), data: json!({"id":P,"balance":403.36}), ..original.clone() };
        let seed = json!({"observedAt":"seed-time","runId":"run","availableCash":"70850.430000"});
        let scope = Scope { tournament_id:T.into(), tournament_slug:"cup".into(), exchanges:Default::default() };
        let manifest = json!("a".repeat(64));
        let owner = json!({"bankScope":{"profileId":P,"runId":"run","bankManifestHash":manifest,"tournamentId":T,"tournamentSlug":"cup"}});
        (original,current,seed,scope,manifest,owner)
    }
    #[test] fn legacy_namespace_uses_independent_durable_identity_and_exact_cash() {
        let (o,c,s,scope,m,r)=fixture();
        assert!(validate(&o,&c,&s,&scope,&m,Some(&r)).is_ok());
        assert!(validate(&o,&c,&s,&scope,&m,None).is_err());
        for field in ["profileId","runId","bankManifestHash","tournamentId","tournamentSlug"] {
            let mut changed=r.clone(); changed["bankScope"][field]=json!("changed");
            assert!(validate(&o,&c,&s,&scope,&m,Some(&changed)).is_err(),"{field}");
        }
        for (field,value) in [("myBalance",json!(70850.430001)),("myBalance",Value::Null),("myBalance",json!("70850.43")),("id",json!(P)),("slug",json!("wrong"))] {
            let mut changed=o.clone();changed.data[field]=value;
            assert!(validate(&changed,&c,&s,&scope,&m,Some(&r)).is_err(),"{field}");
        }
        let mut changed=o.clone();changed.received_at="wrong".into();assert!(validate(&changed,&c,&s,&scope,&m,Some(&r)).is_err());
        changed=o.clone();changed.endpoint="/unrelated".into();assert!(validate(&changed,&c,&s,&scope,&m,Some(&r)).is_err());
        let mut changed=c.clone();changed.status=503;assert!(validate(&o,&changed,&s,&scope,&m,Some(&r)).is_err());
    }
    #[test] fn original_profile_seed_retains_same_profile_check() {
        let (_,c,s,scope,m,r)=fixture();let mut o=c.clone();o.received_at="seed-time".into();
        assert!(validate(&o,&c,&s,&scope,&m,None).is_ok());
        assert!(validate(&o,&c,&s,&scope,&m,Some(&r)).is_ok());
        o.data["id"]=json!(T);assert!(validate(&o,&c,&s,&scope,&m,Some(&r)).is_err());
    }
}
