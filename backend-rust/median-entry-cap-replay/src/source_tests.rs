use super::*;

fn iso(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at).unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn bank() -> V {
    let mut universe = json!({
        "tournamentId": "bda92870-621e-47b0-bc3c-3602c5c26f55",
        "exchanges": [
            {"marketId":"1","exchangeId":"101","option":"YES"},
            {"marketId":"2","exchangeId":"102","option":"YES"}
        ]
    });
    universe["hash"] = json!(digest(&universe));
    json!({"schemaVersion":1,"mode":"fixed_rolling_median_prospective_paper_v1",
        "universe":universe,"thresholds":[1,2,4],"policy":fixed_policy(),"featurePolicy":feature_policy()})
}

fn resign(rows: &mut [V]) {
    let mut previous = String::new();
    for (index, row) in rows.iter_mut().enumerate() {
        row.as_object_mut().unwrap().shift_remove("hash");
        row["sequence"] = json!(index + 1);
        row["previous"] = json!(previous);
        let hash = digest(row);
        row["hash"] = json!(hash);
        previous = hash;
    }
}

fn accounts() -> (V, Vec<V>) {
    let bank = bank();
    let mut rows = [(10_000, 9000, 9500), (11_000, 10_000, 10_500)].map(|(at, p, o)| json!({
        "sequence":0,"previous":"","at":iso(at),"state":null,
        "event":{"type":"market_account_eligibility","snapshot":{
            "tournamentId":bank["universe"]["tournamentId"],"universeHash":bank["universe"]["hash"],
            "positionsAt":iso(p),"ordersAt":iso(o),"expiresAt":iso(p+20_000),
            "excludedMarketIds":["1"],"complete":true
        }}
    })).to_vec();
    resign(&mut rows);
    (bank, rows)
}

fn prices() -> (V, Vec<V>) {
    let bank = bank();
    let mut rows = [10_000, 11_000].map(|at| json!({
        "sequence":0,"previous":"","at":iso(at),"state":null,
        "event":{"type":"market_price_observation","snapshot":{
            "tournamentId":bank["universe"]["tournamentId"],"universeHash":bank["universe"]["hash"],
            "requestStartedAt":iso(at-1000),"receivedAt":iso(at-500),"sourceTimestamp":null,
            "depthAvailable":false,"executable":false,"missingIds":["102"],
            "quotes":[{"exchangeId":"101","marketId":"1","option":"YES","latestPrice":0.5,
                "bestBid":0.5,"bestAsk":0.505,"spread":0.005,"crossed":false}]
        }}
    })).to_vec();
    resign(&mut rows);
    (bank, rows)
}

#[test]
fn normalized_scoped_sources_and_equal_account_clocks_are_accepted() {
    let (bank, mut rows) = accounts();
    source::validate_bank_scope(&bank).unwrap();
    source::validate_accounts(&rows, &bank["universe"]).unwrap();
    let prior = rows[0]["event"]["snapshot"].clone();
    rows[1]["event"]["snapshot"] = prior;
    resign(&mut rows);
    source::validate_accounts(&rows, &bank["universe"]).unwrap();
    let (bank, rows) = prices();
    source::validate_prices(&rows, &bank["universe"]).unwrap();
}

#[test]
fn each_account_projection_clock_must_not_roll_back() {
    for (p, o) in [(6000, 7000), (8999, 10_500), (9000, 9499)] {
        let (bank, mut rows) = accounts();
        let snapshot = &mut rows[1]["event"]["snapshot"];
        snapshot["positionsAt"] = json!(iso(p));
        snapshot["ordersAt"] = json!(iso(o));
        snapshot["expiresAt"] = json!(iso(p + 20_000));
        resign(&mut rows);
        assert_eq!(source::validate_accounts(&rows, &bank["universe"]).unwrap_err(), "ACCOUNT_CHAIN_SCOPE_CLOCK");
    }
}

#[test]
fn exclusions_are_unique_market_ids_not_exchange_ids_or_arbitrary_values() {
    for excluded in [json!(["FOREIGN"]), json!(["3"]), json!(["101"]), json!(["1","1"]),
        json!([1]), json!([null]), json!(["01"]), json!(["9007199254740992"]), json!("1"), V::Null] {
        let (bank, mut rows) = accounts();
        rows[1]["event"]["snapshot"]["excludedMarketIds"] = excluded;
        resign(&mut rows);
        assert_eq!(source::validate_accounts(&rows, &bank["universe"]).unwrap_err(), "ACCOUNT_EXCLUSIONS");
    }
}

#[test]
fn account_scope_shape_and_canonical_clocks_are_required_even_with_correct_hashes() {
    for kind in ["tournament", "universe", "extra_exchange_ids", "event_extra", "state_missing", "offset_clock"] {
        let (bank, mut rows) = accounts();
        match kind {
            "tournament" => rows[1]["event"]["snapshot"]["tournamentId"] = json!("foreign"),
            "universe" => rows[1]["event"]["snapshot"]["universeHash"] = json!("a".repeat(64)),
            "extra_exchange_ids" => rows[1]["event"]["snapshot"]["excludedExchangeIds"] = json!(["101"]),
            "event_extra" => rows[1]["event"]["extra"] = json!(true),
            "state_missing" => { rows[1].as_object_mut().unwrap().shift_remove("state"); },
            "offset_clock" => rows[1]["event"]["snapshot"]["ordersAt"] = json!("1970-01-01T00:00:10.500+00:00"),
            _ => unreachable!()
        }
        resign(&mut rows);
        assert!(source::validate_accounts(&rows, &bank["universe"]).is_err(), "{kind}");
    }
}

#[test]
fn bank_requires_original_policy_and_hash_bound_unique_yes_mapping() {
    for kind in ["hash", "tournament", "policy", "duplicate_market", "duplicate_exchange", "numeric_id", "zero_id", "option"] {
        let mut bank = bank();
        match kind {
            "hash" => bank["universe"]["hash"] = json!("a".repeat(64)),
            "tournament" => bank["universe"]["tournamentId"] = json!("11111111-1111-1111-1111-111111111111"),
            "policy" => bank["policy"]["extra"] = json!(true),
            "duplicate_market" => bank["universe"]["exchanges"][1]["marketId"] = json!("1"),
            "duplicate_exchange" => bank["universe"]["exchanges"][1]["exchangeId"] = json!("101"),
            "numeric_id" => bank["universe"]["exchanges"][1]["marketId"] = json!(2),
            "zero_id" => bank["universe"]["exchanges"][1]["exchangeId"] = json!("0102"),
            "option" => bank["universe"]["exchanges"][1]["option"] = json!("NO"),
            _ => unreachable!()
        }
        if kind != "hash" {
            let mut body = bank["universe"].clone();
            body.as_object_mut().unwrap().shift_remove("hash");
            bank["universe"]["hash"] = json!(digest(&body));
        }
        assert!(source::validate_bank_scope(&bank).is_err(), "{kind}");
    }
}

#[test]
fn bulk_exchange_market_pairs_and_missing_ids_are_exactly_scoped() {
    for kind in ["wrong_market", "wrong_exchange", "wrong_option", "duplicate_quote", "quoted_missing",
        "duplicate_missing", "foreign_missing", "numeric_missing", "missing_array", "empty_batch"] {
        let (bank, mut rows) = prices();
        let snapshot = &mut rows[1]["event"]["snapshot"];
        match kind {
            "wrong_market" => snapshot["quotes"][0]["marketId"] = json!("2"),
            "wrong_exchange" => snapshot["quotes"][0]["exchangeId"] = json!("102"),
            "wrong_option" => snapshot["quotes"][0]["option"] = json!("NO"),
            "duplicate_quote" => { let quote = snapshot["quotes"][0].clone(); snapshot["quotes"].as_array_mut().unwrap().push(quote); },
            "quoted_missing" => snapshot["missingIds"] = json!(["101"]),
            "duplicate_missing" => snapshot["missingIds"] = json!(["102", "102"]),
            "foreign_missing" => snapshot["missingIds"] = json!(["103"]),
            "numeric_missing" => snapshot["missingIds"] = json!([102]),
            "missing_array" => { snapshot.as_object_mut().unwrap().shift_remove("missingIds"); },
            "empty_batch" => { snapshot["quotes"] = json!([]); snapshot["missingIds"] = json!([]); },
            _ => unreachable!()
        }
        resign(&mut rows);
        assert!(source::validate_prices(&rows, &bank["universe"]).is_err(), "{kind}");
    }
}

#[test]
fn bulk_receipt_cannot_roll_back_and_prices_are_typed_and_bounded() {
    for kind in ["rollback", "future", "extra", "string_price", "negative_price", "over_one", "bad_spread", "crossed"] {
        let (bank, mut rows) = prices();
        let snapshot = &mut rows[1]["event"]["snapshot"];
        match kind {
            "rollback" => { snapshot["receivedAt"] = json!(iso(9499)); snapshot["requestStartedAt"] = json!(iso(9000)); },
            "future" => snapshot["receivedAt"] = json!(iso(11_001)),
            "extra" => snapshot["quotes"][0]["extra"] = json!(true),
            "string_price" => snapshot["quotes"][0]["bestBid"] = json!("0.500000"),
            "negative_price" => snapshot["quotes"][0]["latestPrice"] = json!(-0.1),
            "over_one" => snapshot["quotes"][0]["bestAsk"] = json!(1.01),
            "bad_spread" => snapshot["quotes"][0]["spread"] = json!(-1.01),
            "crossed" => snapshot["quotes"][0]["crossed"] = json!(true),
            _ => unreachable!()
        }
        resign(&mut rows);
        assert!(source::validate_prices(&rows, &bank["universe"]).is_err(), "{kind}");
    }
}

#[test]
fn correctly_sealed_malformed_account_prefix_is_rejected_before_replay() {
    let (bank, mut rows) = accounts();
    let snapshot = &mut rows[1]["event"]["snapshot"];
    snapshot["positionsAt"] = json!(iso(6000));
    snapshot["ordersAt"] = json!(iso(7000));
    snapshot["expiresAt"] = json!(iso(26_000));
    snapshot["excludedMarketIds"] = json!(["FOREIGN"]);
    resign(&mut rows);
    let bytes = rows.iter().map(|row| format!("{}\n", serde_json::to_string(row).unwrap())).collect::<String>();
    let path = std::env::temp_dir().join(format!("baal-entry-cap-malformed-{}-{}.jsonl", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::write(&path, &bytes).unwrap();
    let pin = Pin { path: path.clone(), bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())) };
    let sealed = source::rows(&pin).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(source::validate_accounts(&sealed, &bank["universe"]).unwrap_err(), "ACCOUNT_CHAIN_SCOPE_CLOCK");
}
