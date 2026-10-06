use super::*;

fn quote(at: i64, bid: i128, ask: i128) -> Quote {
    Quote {
        market: "1".into(), exchange: "101".into(), at, request: at - 100,
        received: at - 10, sequence: at, version: at.to_string(),
        bids: vec![(bid, 1000)], asks: vec![(ask, 1000)],
    }
}

fn warm(case: Case, median: i128) -> Arm {
    let mut arm = Arm::new(case);
    for index in 0..=61 { arm.wallet.feature_bid("1", index * 30_000, median).unwrap(); }
    arm
}

fn observe(arm: &mut Arm, q: Quote) {
    arm.observe(q, true, None, "synthetic-test-row").unwrap();
}

#[test]
fn exact_one_tick_margin_is_reserved_and_preserved_in_later_fill() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    observe(&mut arm, quote(1_910_000, 490_000, 495_000));
    assert_eq!(arm.wallet.cash, 100 * SCALE);
    assert_eq!(arm.wallet.pending["1"].limit, 495_000);
    assert_eq!(arm.wallet.reserved(""), 495_000);
    assert_eq!(arm.proposals[0]["originalCausalMedianBid"], "0.500000");
    assert_eq!(arm.proposals[0]["originalProtectiveLimit"], "0.500000");
    assert_eq!(arm.proposals[0]["fixedBuyLimit"], "0.495000");
    observe(&mut arm, quote(1_915_000, 495_000, 500_000));
    assert!(arm.wallet.fills.is_empty());
    assert_eq!(arm.wallet.pending["1"].limit, 495_000);
    observe(&mut arm, quote(1_920_000, 490_000, 495_000));
    assert_eq!(arm.wallet.positions["1"].cost, 495_000);
    assert_eq!(arm.wallet.cash, 99_505_000);
    assert_eq!(arm.wallet.buys, 495_000);
    assert_eq!(arm.wallet.fills[0]["protectiveLimit"], "0.495000");
    assert_eq!(arm.proposals.len(), 1);
    arm.wallet.assert_funded().unwrap();
}

#[test]
fn native_control_can_fill_the_price_that_capped_arm_blocks() {
    let mut baseline = warm(Case::Baseline, 500_000);
    let mut capped = warm(Case::OriginalReferenceCap, 500_000);
    for arm in [&mut baseline, &mut capped] {
        observe(arm, quote(1_910_000, 490_000, 495_000));
        observe(arm, quote(1_915_000, 495_000, 500_000));
    }
    assert_eq!(baseline.wallet.positions["1"].cost, 500_000);
    assert!(capped.wallet.positions.is_empty());
    assert_eq!(capped.wallet.reserved(""), 495_000);
}

#[test]
fn latency_independence_and_individual_level_depth_remain_required() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    observe(&mut arm, quote(1_910_000, 490_000, 495_000));
    observe(&mut arm, quote(1_913_099, 490_000, 495_000)); // request is one ms too early
    assert!(arm.wallet.fills.is_empty());
    let mut repeated = quote(1_914_000, 490_000, 495_000);
    repeated.sequence = 1_910_000;
    observe(&mut arm, repeated);
    assert!(arm.wallet.fills.is_empty());
    let mut thin = quote(1_915_000, 485_000, 490_000);
    thin.asks = vec![(490_000, 99), (495_000, 99), (500_000, 1000)];
    observe(&mut arm, thin);
    assert!(arm.wallet.fills.is_empty()); // neither aggregate thin levels nor over-cap depth count
    let mut enough = quote(1_916_000, 485_000, 490_000);
    enough.asks = vec![(490_000, 99), (495_000, 100)];
    observe(&mut arm, enough);
    assert_eq!(arm.wallet.positions["1"].cost, 495_000);
}

#[test]
fn filled_price_and_exit_accounting_use_actual_observed_cost() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    observe(&mut arm, quote(1_910_000, 490_000, 495_000));
    observe(&mut arm, quote(1_915_000, 485_000, 490_000));
    assert_eq!(arm.wallet.positions["1"].cost, 490_000);
    observe(&mut arm, quote(1_920_000, 495_000, 500_000));
    assert_eq!(arm.wallet.pending["1"].limit, 495_000);
    assert!(!arm.wallet.pending["1"].buy);
    observe(&mut arm, quote(1_925_000, 495_000, 500_000));
    assert!(arm.wallet.positions.is_empty());
    assert_eq!(arm.wallet.realized, 5000);
    assert_eq!(arm.wallet.cash, 100_005_000);
    assert_eq!(arm.wallet.buys, 490_000);
    assert_eq!(arm.proposals.len(), 1); // the exit never becomes an entry-cap proposal
    arm.wallet.assert_funded().unwrap();
}

#[test]
fn carried_cap_does_not_follow_new_reference_even_after_checkpoint_restore() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    observe(&mut arm, quote(1_910_000, 490_000, 495_000));
    arm.wallet = MedianWallet::restore(arm.wallet.checkpoint().unwrap(), 1).unwrap();
    for index in 0..70 { arm.wallet.feature_bid("1", 1_911_000 + index * 100, 800_000).unwrap(); }
    observe(&mut arm, quote(1_920_000, 495_000, 500_000));
    assert!(arm.wallet.positions.is_empty());
    assert_eq!(arm.wallet.pending["1"].limit, 495_000);
    assert_eq!(arm.proposals.len(), 1);
    assert_eq!(arm.proposals[0]["originalCausalMedianBid"], "0.500000");
    observe(&mut arm, quote(1_925_000, 490_000, 495_000));
    assert_eq!(arm.wallet.positions["1"].cost, 495_000);
}

#[test]
fn replacement_after_original_expiry_freezes_a_new_proposal_not_a_reprice() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    observe(&mut arm, quote(1_910_000, 490_000, 495_000));
    observe(&mut arm, quote(2_030_001, 485_000, 490_000));
    assert!(arm.wallet.fills.is_empty());
    assert_eq!(arm.proposals.len(), 2);
    assert_eq!(arm.wallet.pending["1"].created, 2_030_001);
    assert_eq!(arm.wallet.pending["1"].limit, 495_000);
    assert_eq!(arm.wallet.counts["intent_expired"], 1);
}

#[test]
fn future_cutoff_primes_history_only_and_rejects_straddling_request() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    let cutoff = 1_920_000;
    for q in [quote(1_910_000, 490_000, 495_000), quote(1_915_000, 490_000, 495_000)] {
        arm.observe(q, true, Some(cutoff), "before-cutoff").unwrap();
    }
    assert_eq!(arm.wallet.cash, 100 * SCALE);
    assert!(arm.wallet.fills.is_empty() && arm.wallet.pending.is_empty() && arm.proposals.is_empty());
    arm.observe(quote(cutoff, 490_000, 495_000), true, Some(cutoff), "at-cutoff").unwrap();
    let mut straddling = quote(cutoff + 100, 490_000, 495_000);
    straddling.request = cutoff - 1;
    arm.observe(straddling, true, Some(cutoff), "straddling").unwrap();
    assert!(arm.wallet.pending.is_empty());
    arm.observe(quote(cutoff + 5000, 490_000, 495_000), true, Some(cutoff), "after-cutoff").unwrap();
    assert_eq!(arm.wallet.pending.len(), 1);
    assert_eq!(arm.proposals.len(), 1);
    assert_eq!(arm.wallet.cash, 100 * SCALE);
}

#[test]
fn original_baseline_checkpoint_parity_after_every_observation() {
    let mut arm = warm(Case::Baseline, 500_000);
    let mut original = arm.wallet.clone();
    for (q, eligible) in [
        (quote(1_910_000, 490_000, 495_000), true),
        (quote(1_915_000, 495_000, 500_000), true),
        (quote(1_920_000, 510_000, 515_000), true),
        (quote(1_925_000, 475_000, 480_000), true),
        (quote(1_930_000, 475_000, 480_000), false),
        (quote(1_935_000, 475_000, 480_000), true),
        (quote(1_940_000, 475_000, 480_000), true),
        (quote(2_900_000, 490_000, 495_000), true),
    ] {
        original.observe_with_entry_permission(q.clone(), eligible, true).unwrap();
        arm.observe(q, eligible, None, "parity").unwrap();
        assert_eq!(arm.wallet.checkpoint().unwrap(), original.checkpoint().unwrap());
    }
    original.boundary(None);
    arm.wallet.boundary(None);
    assert_eq!(arm.wallet.checkpoint().unwrap(), original.checkpoint().unwrap());
}

#[test]
fn nonbinding_cap_is_exact_wallet_parity_with_original_control() {
    let mut original = warm(Case::Baseline, 510_000);
    let mut capped = warm(Case::OriginalReferenceCap, 510_000);
    for q in [quote(1_910_000, 490_000, 495_000), quote(1_915_000, 495_000, 500_000),
        quote(1_920_000, 510_000, 515_000), quote(1_925_000, 510_000, 515_000)] {
        observe(&mut original, q.clone());
        observe(&mut capped, q);
        assert_eq!(original.wallet.checkpoint().unwrap(), capped.wallet.checkpoint().unwrap());
    }
    assert_eq!(capped.proposals[0]["lowered"], false);
}

#[test]
fn no_followup_stays_pending_and_unknown_or_stale_account_cannot_fill() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    observe(&mut arm, quote(1_910_000, 490_000, 495_000));
    let report = arm.report(1_910_000).unwrap();
    assert_eq!(report["wallet"]["pendingIntents"], 1);
    assert_eq!(report["wallet"]["realizedPnl"], "0.000000");
    assert!(report["wallet"]["fills"].as_array().unwrap().is_empty());
    arm.observe(quote(1_915_000, 490_000, 495_000), false, None, "unknown-account").unwrap();
    assert!(arm.wallet.fills.is_empty() && arm.wallet.pending.is_empty());
    observe(&mut arm, quote(1_920_000, 490_000, 495_000));
    let mut stale = quote(1_950_000, 490_000, 495_000);
    stale.received = 1_929_999;
    stale.request = 1_929_900;
    observe(&mut arm, stale);
    assert!(arm.wallet.fills.is_empty() && arm.wallet.pending.is_empty());
}

#[test]
fn causal_reference_omits_future_and_newest_before_freezing_cap() {
    let mut arm = warm(Case::OriginalReferenceCap, 500_000);
    arm.wallet.feature_bid("1", 1_909_800, 900_000).unwrap(); // newest eligible must be dropped
    arm.wallet.feature_bid("1", 1_909_950, 100_000).unwrap(); // after request, never eligible
    observe(&mut arm, quote(1_910_000, 490_000, 495_000));
    assert_eq!(arm.proposals[0]["lastReferenceKnownAtMs"], 1_830_000);
    assert_eq!(arm.proposals[0]["referencePoints"], 62);
    assert_eq!(arm.proposals[0]["originalCausalMedianBid"], "0.500000");
}

#[test]
fn account_lookup_remains_causal_when_cross_market_requests_go_backwards() {
    let iso = |at| chrono::DateTime::from_timestamp_millis(at).unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let accounts = vec![
        json!({"at":iso(70),"event":{"snapshot":{"excludedMarketIds":["1"]}}}),
        json!({"at":iso(90),"event":{"snapshot":{"excludedMarketIds":[]}}})
    ];
    assert_eq!(source::causal_account(&accounts, 100, 110).unwrap()["excludedMarketIds"], json!([]));
    assert_eq!(source::causal_account(&accounts, 80, 120).unwrap()["excludedMarketIds"], json!(["1"]));
    assert!(source::causal_account(&accounts, 60, 120).is_none());
}
