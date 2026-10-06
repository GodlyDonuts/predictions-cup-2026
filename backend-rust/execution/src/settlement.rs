// execution/settlement-controller.mjs + live-paper/native-publication.mjs +
// live-paper/settlement-value-native.mjs.
use crate::controller::{entry_window_gate, CompetitionController, ControllerOptions};
use crate::core::{conservative_cash_floor, payload_hash, prepare_intent};
use crate::production::{probability_receipt_policy, validate_production_activation};
use baal_service_runtime::fixed::amount;
use baal_service_runtime::json_text::{digest, js_int, js_stringify, sha256};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const NEED: &str = "SETTLEMENT_EXECUTION_";
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("{NEED}{code}"))
    }
}
fn units(v: &V) -> i128 {
    amount(&crate::js_key(v)).unwrap_or(i128::MIN)
}
fn instant(v: &V) -> i64 {
    crate::controller::parse(v)
}
fn iso(n: i64) -> String {
    crate::controller::iso(n)
}
fn hex64(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
pub const RECEIPT_PRODUCTION_POLICY_JSON: &str =
    r#"{"schemaVersion":1,"mode":"receipt_production_policy_v1","maximumOrders":3,"fillsPerOrder":1,"accountQueue":"serialized_account_queue_v1","accountReadsPerCycle":3,"settlementOnly":true}"#;

pub fn validate_settlement_bank(bank: &V, activation: &V, universe: &V) -> Result<V, String> {
    validate_production_activation(activation).map_err(|e| format!("{NEED}{e}"))?;
    let receipt = activation.get("strategyId").and_then(V::as_str) == Some("probability-receipt-native-v1");
    need(
        bank.get("schemaVersion").and_then(V::as_i64) == Some(if receipt { 2 } else { 1 })
            && bank.get("mode").and_then(V::as_str)
                == Some(if receipt {
                    "baal_probability_receipt_production_v1"
                } else {
                    "baal_settlement_production_v1"
                })
            && digest(bank) == activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
        "BANK",
    )?;
    if receipt {
        need(
            bank.get("candidates").and_then(V::as_array).map(|a| a.len() <= 3).unwrap_or(false)
                && hex64(bank.pointer("/paperBinding/bankHash").unwrap_or(&V::Null))
                && bank.pointer("/paperBinding/policyHash").and_then(V::as_str)
                    == Some(digest(&probability_receipt_policy()).as_str()),
            "PAPER_BINDING",
        )?;
        need(
            bank.get("receiptPolicy").map(digest)
                == serde_json::from_str::<V>(RECEIPT_PRODUCTION_POLICY_JSON).ok().as_ref().map(digest)
                && bank.pointer("/policy/minimumEdge").and_then(V::as_str) == Some("0.020000")
                && bank.pointer("/policy/maximumSpread").and_then(V::as_str) == Some("0.020000"),
            "RECEIPT_POLICY",
        )?;
    }
    need(
        bank.get("candidates").and_then(V::as_array).is_some_and(|a| {
            !a.is_empty()
                && a.len() <= 600
                && a.iter()
                    .map(|c| c.get("id").and_then(V::as_str).unwrap_or(""))
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == a.len()
        }),
        "CANDIDATES",
    )?;
    for c in bank.get("candidates").and_then(V::as_array).cloned().unwrap_or_default() {
        need(
            c.get("side").and_then(V::as_str) == Some("YES")
                && c.get("id").and_then(V::as_str)
                    == Some(format!("{}:YES", c.get("marketId").and_then(V::as_str).unwrap_or("")).as_str())
                && universe.get("exchanges").and_then(V::as_array).is_some_and(|a| {
                    a.iter().any(|x| {
                        x.get("marketId") == c.get("marketId") && x.get("exchangeId") == c.get("exchangeId")
                    })
                })
                && hex64(c.get("predicateHash").unwrap_or(&V::Null))
                && js_int(c.get("settlesAtMs").unwrap_or(&V::Null)).is_some()
                && c.get("riskGroups").and_then(V::as_array).is_some_and(|a| !a.is_empty()),
            "SCOPE",
        )?;
    }
    need(
        bank.get("contracts").and_then(V::as_object).is_some()
            && bank.get("candidates").and_then(V::as_array).is_some_and(|a| {
                a.iter().all(|c| {
                    let contract = bank
                        .get("contracts")
                        .and_then(V::as_object)
                        .and_then(|o| o.get(c.get("marketId").and_then(V::as_str).unwrap_or("")))
                        .cloned()
                        .unwrap_or(V::Null);
                    hex64(contract.get("rootHash").unwrap_or(&V::Null))
                        && contract.get("predicateHash") == c.get("predicateHash")
                        && contract.get("exchangeId") == c.get("exchangeId")
                })
            }),
        "CONTRACT_BINDINGS",
    )?;
    let p = bank.get("policy").cloned().unwrap_or(V::Null);
    need(
        p.get("maxOrderCost") == activation.get("maxOrderCost")
            && p.get("maxExposure") == activation.get("maxAggregateCost")
            && p.get("cumulativeBuyBudget") == activation.get("maxCumulativeBuyCost")
            && p.get("cashReserve") == activation.get("cashReserve")
            && p.get("depthFractionBps").and_then(V::as_i64) == Some(100)
            && p.get("maximumBookAgeMs").and_then(V::as_i64) == Some(20000)
            && p.get("maximumResearchAgeMs").and_then(V::as_i64) == Some(21600000)
            && p.get("slippageTicks").and_then(V::as_i64) == Some(1)
            && p.get("fractionalKellyBps").and_then(V::as_i64).is_some_and(|n| n <= 2500)
            && units(p.get("minimumEdge").unwrap_or(&V::Null)) >= 20000
            && units(p.get("maximumSpread").unwrap_or(&V::Null)) <= 50000,
        "POLICY",
    )?;
    Ok(bank.clone())
}

// native-publication.mjs: executable identity is pinned by content hash.
pub fn native_publication_binding(runtime_dir: &Path, binding: &V) -> Result<PathBuf, String> {
    need(
        runtime_dir.is_absolute()
            && binding.get("schemaVersion").and_then(V::as_i64) == Some(1)
            && binding.get("mode").and_then(V::as_str) == Some("native_public_book_prefix_v1")
            && ["binaryHash", "engineHash", "runtimeLibraryHash"]
                .iter()
                .all(|k| hex64(binding.get(k).unwrap_or(&V::Null))),
        "BINDING",
    )?;
    let root = runtime_dir
        .join("native-builds")
        .join(binding.get("binaryHash").and_then(V::as_str).unwrap_or(""));
    let executable = root.join("baal-backend.exe");
    need(
        std::fs::read(&executable)
            .map(|b| sha256(&b) == binding.get("binaryHash").and_then(V::as_str).unwrap_or(""))
            .unwrap_or(false)
            && std::fs::read(root.join("libunwind.dll"))
                .map(|b| sha256(&b) == binding.get("runtimeLibraryHash").and_then(V::as_str).unwrap_or(""))
                .unwrap_or(false),
        "BINARY_CHANGED",
    )?;
    Ok(executable)
}

// settlement-value-native.mjs runSettlementValue: write the input, run the
// pinned binary, verify its output against the declared identity.
pub fn run_settlement_value(input: &V, dir: &Path, executable: &Path) -> Result<V, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("SETTLEMENT_ADAPTER_NATIVE_REJECTED:{e}"))?;
    let file = dir.join("input.json");
    let out = dir.join("output.json");
    let bytes = format!("{}\n", js_stringify(input));
    let binary_hash = sha256(&std::fs::read(executable).map_err(|e| format!("SETTLEMENT_ADAPTER_NATIVE_REJECTED:{e}"))?);
    // flag 'wx' — exclusive create.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .map_err(|e| format!("SETTLEMENT_ADAPTER_NATIVE_REJECTED:{e}"))?;
        f.write_all(bytes.as_bytes())
            .map_err(|e| format!("SETTLEMENT_ADAPTER_NATIVE_REJECTED:{e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.sync_all();
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
        }
    }
    let input_sha = sha256(bytes.as_bytes());
    let start = std::time::Instant::now();
    let child = std::process::Command::new(executable)
        .args(["settlement-policy", "--input"])
        .arg(&file)
        .args(["--expected-sha", &input_sha, "--out"])
        .arg(&out)
        .output()
        .map_err(|e| format!("SETTLEMENT_ADAPTER_NATIVE_REJECTED:{e}"))?;
    if !child.status.success() {
        return Err("SETTLEMENT_ADAPTER_NATIVE_REJECTED".into());
    }
    let result: V = serde_json::from_str(
        &std::fs::read_to_string(&out).map_err(|e| format!("SETTLEMENT_ADAPTER_OUTPUT:{e}"))?,
    )
    .map_err(|e| format!("SETTLEMENT_ADAPTER_OUTPUT:{e}"))?;
    if result.get("inputHash").and_then(V::as_str) != Some(input_sha.as_str())
        || sha256(&std::fs::read(executable).unwrap_or_default()) != binary_hash
        || result.get("executionAuthority") != Some(&V::Bool(false))
        || result.get("fills").and_then(V::as_i64) != Some(0)
        || result.get("competitionOrders").and_then(V::as_i64) != Some(0)
    {
        return Err("SETTLEMENT_ADAPTER_OUTPUT".into());
    }
    Ok(json!({
        "result": result,
        "binaryHash": binary_hash,
        "inputHash": input_sha,
        "outputHash": sha256(&std::fs::read(&out).unwrap_or_default()),
        "wallMs": start.elapsed().as_secs_f64() * 1000.0,
    }))
}

pub struct SettlementState {
    pub bank: V,
    pub bank_file: PathBuf,
    pub executable: PathBuf,
    pub receipt_sequence: i64,
    pub latest_native: V,
    pub checked_contracts: std::collections::BTreeMap<String, V>,
    pub last_freshness_repair: V,
    pub prepared_settlement_book: Option<V>,
}

pub struct SettlementCompetitionController {
    pub base: CompetitionController,
    pub s: Rc<RefCell<SettlementState>>,
    pub runtime_dir: PathBuf,
}

impl SettlementCompetitionController {
    pub fn new(options: ControllerOptions) -> Result<Self, String> {
        validate_production_activation(&options.activation).map_err(|e| format!("{NEED}{e}"))?;
        let mut base = CompetitionController::new(options)?;
        let runtime_dir = base.runtime_dir.clone();
        let activation = base.activation.clone();
        let result = (|| -> Result<Rc<RefCell<SettlementState>>, String> {
            let bank_file = runtime_dir.join("production-banks").join(format!(
                "{}.json",
                activation.get("strategyBankHash").and_then(V::as_str).unwrap_or("")
            ));
            let bank = validate_settlement_bank(
                &serde_json::from_str::<V>(
                    &std::fs::read_to_string(&bank_file).map_err(|e| format!("{NEED}BANK:{e}"))?,
                )
                .map_err(|e| format!("{NEED}BANK:{e}"))?,
                &activation,
                &base.universe,
            )?;
            let executable = native_publication_binding(&runtime_dir, bank.get("nativeBackend").unwrap())?;
            let s = Rc::new(RefCell::new(SettlementState {
                bank,
                bank_file,
                executable,
                receipt_sequence: 0,
                latest_native: V::Null,
                checked_contracts: std::collections::BTreeMap::new(),
                last_freshness_repair: V::Null,
                prepared_settlement_book: None,
            }));
            Ok(s)
        })();
        let s = match result {
            Ok(s) => s,
            Err(e) => {
                let _ = base.close();
                return Err(e);
            }
        };
        // Install the settlement overrides: bootstrap id, admit hook, gate.
        {
            let (s2, rt, act) = (s.clone(), runtime_dir.clone(), activation.clone());
            base.inner.borrow_mut().entry_reference_gate = Some(Rc::new(
                move |inner, _activation, _runtime_dir, i: &V, permit: &V, now_ms: i64| {
                    settlement_reference_gate(&s2, inner, &rt, &act, i, permit, now_ms)
                },
            ));
            let first = s
                .borrow()
                .bank
                .pointer("/candidates/0/marketId")
                .and_then(V::as_str)
                .unwrap_or("")
                .to_string();
            base.inner.borrow_mut().bootstrap_market_id = Some(Rc::new(move || first.clone()));
            let s3 = s.clone();
            base.inner.borrow_mut().pre_admit = Some(Rc::new(move |inner| {
                let book = s3.borrow().prepared_settlement_book.clone();
                if let (Some(_), Some(book)) = (inner.borrow().current_permit.as_ref(), book) {
                    inner
                        .borrow_mut()
                        .current_permit
                        .as_mut()
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .insert("settlementBook".into(), book);
                }
            }));
        }
        Ok(Self { base, s, runtime_dir })
    }
    pub fn eligible_market_ids(&self) -> Vec<String> {
        let now = self.base.now_ms();
        let s = self.s.borrow();
        s.bank
            .get("candidates")
            .and_then(V::as_array)
            .map(|a| {
                a.iter()
                    .filter(|c| {
                        let r = c.get("research").cloned().unwrap_or(V::Null);
                        r.get("settlementReviewed") == Some(&V::Bool(true))
                            && r.get("flags").and_then(V::as_array).map(|f| f.is_empty()).unwrap_or(false)
                            && r.get("recordedAtMs").and_then(V::as_i64).map(|t| t <= now).unwrap_or(false)
                            && r.get("sourceAtMs").and_then(V::as_i64).map(|t| t <= now).unwrap_or(false)
                            && r.get("expiresAtMs").and_then(V::as_i64).map(|t| now < t).unwrap_or(false)
                            && r.get("sourceAtMs").and_then(V::as_i64)
                                .map(|t| {
                                    now - t <= s
                                        .bank
                                        .pointer("/policy/maximumResearchAgeMs")
                                        .and_then(V::as_i64)
                                        .unwrap_or(21600000)
                                })
                                .unwrap_or(false)
                    })
                    .filter_map(|c| c.get("marketId").and_then(V::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn before_book(&mut self, market_id: &str) -> Result<(), String> {
        if !self.base.can_write()
            || self.base.active().is_some()
            || !self.base.inner.borrow().selected.contains(&market_id.to_string())
        {
            return Ok(());
        }
        let now = self.base.now_ms();
        let clocks: Vec<i64> = {
            let inner = self.base.inner.borrow();
            [
                inner.cash.as_ref().and_then(|c| c.get("at").cloned()),
                inner.account.as_ref().and_then(|a| a.pointer("/positions/at").cloned()),
                inner.account.as_ref().and_then(|a| a.pointer("/orders/at").cloned()),
            ]
            .iter()
            .map(|v| instant(v.as_ref().unwrap_or(&V::Null)))
            .collect()
        };
        // Keep ten seconds of headroom for the queued book and durable
        // dispatch. Reconcile before requesting the decision book, never after
        // it as if causal.
        if clocks.iter().any(|t| *t == i64::MIN || *t > now || now - *t > 10000) {
            self.base.cash_refresh()?;
            self.base.refresh_account()?;
            self.s.borrow_mut().last_freshness_repair = json!({
                "at": iso(self.base.now_ms()),
                "marketId": market_id,
                "addedReads": 3,
                "networkQueue": "existing_serialized_account_queue",
            });
        }
        Ok(())
    }
    pub fn before_books(&mut self, args: &V) -> Result<(), String> {
        self.s.borrow_mut().checked_contracts.clear();
        self.base.before_books(args)?;
        if !self.base.can_write() || self.base.active().is_some() {
            return Ok(());
        }
        for id in self.base.inner.borrow().selected.clone() {
            let expected = self.s.borrow().bank.pointer(&format!("/contracts/{id}")).cloned().unwrap_or(V::Null);
            if expected.is_null() {
                continue;
            }
            let r = self.base.transport.nodes(&id).map_err(|e| e.code)?;
            if payload_hash(r.pointer("/data/root").unwrap_or(&V::Null)).ok().as_deref()
                != expected.get("rootHash").and_then(V::as_str)
            {
                self.base.inner.borrow_mut().reason = Some("CURRENT_CONTRACT_CHANGED".into());
                continue;
            }
            self.s.borrow_mut().checked_contracts.insert(
                id.clone(),
                json!({
                    "at": r.get("receivedAt").unwrap().clone(),
                    "rootHash": expected.get("rootHash").unwrap().clone(),
                }),
            );
        }
        Ok(())
    }
    pub fn bank_current(&self) -> bool {
        let s = self.s.borrow();
        let bank = std::fs::read_to_string(&s.bank_file)
            .ok()
            .and_then(|f| serde_json::from_str::<V>(&f).ok())
            .map(|b| digest(&b) == self.base.activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""))
            .unwrap_or(false);
        bank && native_publication_binding(&self.runtime_dir, s.bank.get("nativeBackend").unwrap())
            .map(|e| e == s.executable)
            .unwrap_or(false)
    }
    pub fn native_input(&mut self, book: &V, skip_intent: Option<&V>) -> Result<V, String> {
        need(self.bank_current(), "BANK_CHANGED")?;
        let now = self.base.now_ms();
        let state = self.base.inner.borrow().state.clone();
        let intents: Vec<V> = state
            .get("intents")
            .and_then(V::as_object)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default();
        let skip_id = skip_intent.and_then(|i| i.get("intentId")).and_then(V::as_str).unwrap_or("");
        let mut positions = Vec::new();
        for (exchange_id, p) in state.get("owned").and_then(V::as_object).cloned().unwrap_or_default() {
            let entry = intents
                .iter()
                .filter(|i| {
                    i.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str())
                        && i.get("action").and_then(V::as_str) == Some("buy")
                        && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
                })
                .last();
            need(entry.is_some(), "OWNED_SOURCE")?;
            let entry = entry.unwrap();
            positions.push(json!({
                "candidateId": format!("{}:YES", p.get("marketId").and_then(V::as_str).unwrap_or("")),
                "quantity": p.get("quantity").unwrap().clone(),
                "cost": p.get("cost").unwrap().clone(),
                "enteredAtMs": instant(
                    entry
                        .pointer("/reconciledFills/0/filledAt")
                        .or_else(|| entry.get("preparedAt"))
                        .unwrap_or(&V::Null),
                ),
            }));
        }
        let market_id = book.get("marketId").and_then(V::as_str).unwrap_or("");
        let contract = self
            .s
            .borrow()
            .checked_contracts
            .get(market_id)
            .cloned()
            .unwrap_or(V::Null);
        need(
            !contract.is_null()
                && contract.get("rootHash")
                    == self.s.borrow().bank.pointer(&format!("/contracts/{market_id}/rootHash")).cloned().as_ref()
                && instant(contract.get("at").unwrap_or(&V::Null)) <= instant(book.get("requestStartedAt").unwrap())
                && now - instant(contract.get("at").unwrap()) <= 180000,
            "CONTRACT_NOT_CURRENT",
        )?;
        // Maximum attempted purchase principal conservatively consumes the
        // lifetime budget. An intent under its own fence is not counted again.
        let cumulative: i128 = intents
            .iter()
            .filter(|i| {
                i.get("intentId").and_then(V::as_str) != Some(skip_id)
                    && i.get("action").and_then(V::as_str) == Some("buy")
                    && i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
            })
            .map(|i| units(i.get("price").unwrap_or(&V::Null)) * i.get("quantity").and_then(V::as_i64).unwrap_or(0) as i128)
            .sum();
        let reservations: Vec<V> = intents
            .iter()
            .filter(|i| {
                i.get("intentId").and_then(V::as_str) != Some(skip_id)
                    && i.get("action").and_then(V::as_str) == Some("buy")
                    && i.get("status").and_then(V::as_str) == Some("prepared")
            })
            .map(|i| {
                json!({
                    "candidateId": format!("{}:YES", i.get("marketId").and_then(V::as_str).unwrap_or("")),
                    "maximumCost": crate::core::format_micros(
                        units(i.get("price").unwrap_or(&V::Null))
                            * i.get("quantity").and_then(V::as_i64).unwrap_or(0) as i128,
                    ),
                })
            })
            .collect();
        let account = self.base.inner.borrow().account.clone().unwrap_or(V::Null);
        let positions_at = instant(account.pointer("/positions/at").unwrap_or(&V::Null));
        let orders_at = instant(account.pointer("/orders/at").unwrap_or(&V::Null));
        let known_at_ms = positions_at.max(orders_at);
        let positions_complete = account.pointer("/positions/complete") == Some(&V::Bool(true));
        let orders_complete = account.pointer("/orders/complete") == Some(&V::Bool(true));
        let position_rows = account.pointer("/positions/rows").and_then(V::as_array).cloned().unwrap_or_default();
        let order_rows = account.pointer("/orders/rows").and_then(V::as_array).cloned().unwrap_or_default();
        let candidates: Vec<V> = self
            .s
            .borrow()
            .bank
            .get("candidates")
            .and_then(V::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|c| {
                let mut c = c;
                let has_book = c.get("exchangeId") == book.get("exchangeId");
                let book_v = if has_book {
                    json!({
                        "requestStartedAtMs": instant(book.get("requestStartedAt").unwrap_or(&V::Null)),
                        "receivedAtMs": instant(book.get("receivedAt").unwrap_or(&V::Null)),
                        "availableAtMs": book.get("productionAvailableAtMs").unwrap().clone(),
                        "sequence": book.get("productionSequence").unwrap().clone(),
                        "sourceHash": digest(book),
                        "representation": "direct_yes",
                        "bids": book.get("bids").unwrap().clone(),
                        "asks": book.get("asks").unwrap().clone(),
                    })
                } else {
                    V::Null
                };
                let exchange_id = c.get("exchangeId").and_then(V::as_str).unwrap_or("").to_string();
                c.as_object_mut().unwrap().insert("book".into(), book_v);
                c.as_object_mut().unwrap().insert(
                    "account".into(),
                    json!({
                        "positionsAtMs": positions_at,
                        "ordersAtMs": orders_at,
                        "knownAtMs": known_at_ms,
                        "complete": positions_complete && orders_complete,
                        "positionExcluded": position_rows.iter().any(|p| {
                            p.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str())
                                && p.get("quantity").and_then(V::as_i64).unwrap_or(0) != 0
                        }),
                        "openOrderExcluded": order_rows.iter().any(|o| {
                            o.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str())
                        }),
                    }),
                );
                c
            })
            .collect();
        let cash = self.base.inner.borrow().cash.clone().unwrap_or(V::Null);
        let mut input = json!({
            "schemaVersion": 1,
            "mode": "settlement_value_policy_v1",
            "asOfMs": now,
            "policy": self.s.borrow().bank.get("policy").unwrap().clone(),
            "wallet": {
                "capital": self.base.activation.get("sizingCapital").unwrap().clone(),
                "cash": crate::core::format_micros(
                    conservative_cash_floor(cash.pointer("/data/myBalance").unwrap_or(&V::Null))?,
                ),
                "cumulativeBuys": crate::core::format_micros(cumulative),
                "positions": positions,
                "reservations": reservations,
            },
            "candidates": candidates,
        });
        if self.s.borrow().bank.get("mode").and_then(V::as_str) == Some("baal_probability_receipt_production_v1") {
            input
                .as_object_mut()
                .unwrap()
                .insert("schemaVersion".into(), json!(2));
            input
                .as_object_mut()
                .unwrap()
                .insert("mode".into(), json!("probability_receipt_policy_v1"));
            input.as_object_mut().unwrap().insert(
                "receiptPolicy".into(),
                self.s.borrow().bank.get("receiptPolicy").unwrap().clone(),
            );
            let mut last_exited: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
            for i in &intents {
                if i.get("action").and_then(V::as_str) == Some("sell")
                    && i.get("status").and_then(V::as_str) == Some("closed")
                    && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
                    && self
                        .base
                        .inner
                        .borrow()
                        .state
                        .pointer(&format!("/owned/{}", i.get("exchangeId").and_then(V::as_str).unwrap_or("")))
                        .is_none()
                {
                    let id = format!("{}:YES", i.get("marketId").and_then(V::as_str).unwrap_or(""));
                    let at = instant(i.get("closedAt").unwrap_or(&V::Null));
                    last_exited
                        .entry(id)
                        .and_modify(|e| *e = (*e).max(at))
                        .or_insert(at);
                }
            }
            let exits = self.base.inner.borrow().state.pointer("/control/exits").and_then(V::as_object).cloned().unwrap_or_default();
            let risk_exits: serde_json::Map<String, V> = exits
                .into_iter()
                .filter_map(|(_exchange_id, x)| {
                    Some((
                        format!("{}:YES", x.get("marketId").and_then(V::as_str)?),
                        x.get("reason").unwrap().clone(),
                    ))
                })
                .collect();
            input.as_object_mut().unwrap().insert(
                "receiptState".into(),
                json!({
                    "riskExits": risk_exits,
                    "lastExitedAtMs": last_exited,
                }),
            );
        }
        Ok(input)
    }
    pub fn evaluate(&mut self, book: &V, skip_intent: Option<&V>) -> Result<V, String> {
        let input = self.native_input(book, skip_intent)?;
        let dir = self.base.dir.join("native-decisions").join(uuid::Uuid::new_v4().to_string());
        let executable = self.s.borrow().executable.clone();
        let checked = run_settlement_value(&input, &dir, &executable)?;
        need(
            checked.get("binaryHash").and_then(V::as_str)
                == self.s.borrow().bank.pointer("/nativeBackend/binaryHash").and_then(V::as_str)
                && checked.pointer("/result/engineHash").and_then(V::as_str)
                    == self.s.borrow().bank.pointer("/nativeBackend/engineHash").and_then(V::as_str),
            "NATIVE_IDENTITY",
        )?;
        let decision = checked
            .pointer("/result/decisions")
            .and_then(V::as_array)
            .and_then(|a| {
                a.iter()
                    .find(|c| c.get("exchangeId") == book.get("exchangeId"))
                    .cloned()
            })
            .unwrap_or(V::Null);
        need(!decision.is_null(), "DECISION_SCOPE")?;
        self.s.borrow_mut().latest_native = json!({
            "at": iso(self.base.now_ms()),
            "inputHash": checked.get("inputHash").unwrap().clone(),
            "outputHash": checked.get("outputHash").unwrap().clone(),
            "wallMs": checked.get("wallMs").unwrap().clone(),
            "decision": decision.clone(),
            "profitabilityEstablished": false,
        });
        Ok(json!({
            "decision": decision,
            "proof": {
                "bankHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
                "bookHash": digest(book),
                "inputHash": checked.get("inputHash").unwrap().clone(),
                "outputHash": checked.get("outputHash").unwrap().clone(),
            },
        }))
    }
    // submit wraps base.submit but stashes the settlementBook on the prepared
    // slot so the admit fence can attach it to the current permit.
    pub fn submit(&mut self, i: &V, snapshot: Option<&V>, retry: bool, reference_proof: Option<&V>) -> Result<(), String> {
        if let Some(book) = reference_proof.and_then(|p| p.get("settlementBook")) {
            self.s.borrow_mut().prepared_settlement_book = Some(book.clone());
        }
        self.base.submit(i, snapshot, retry, reference_proof)
    }
    pub fn on_book(&mut self, response: &V) -> Result<(), String> {
        let market_id = response
            .get("endpoint")
            .and_then(V::as_str)
            .and_then(|e| {
                let p = e.strip_prefix("/markets/")?;
                let e2 = p.find("/orderbook")?;
                let id = &p[..e2];
                if id.bytes().all(|b| b.is_ascii_digit()) {
                    Some(id.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        if !self.base.can_write()
            || self.base.active().is_some()
            || !self.base.inner.borrow().selected.contains(&market_id)
            || !self.base.inner.borrow().markets.contains_key(&market_id)
            || self.base.now_ms()
                - instant(
                    self.base
                        .inner
                        .borrow()
                        .markets
                        .get(&market_id)
                        .and_then(|m| m.get("at"))
                        .unwrap_or(&V::Null),
                )
                > 180000
        {
            return Ok(());
        }
        let original = self.base.checked_book(response, &market_id)?;
        let mut book = original;
        {
            let mut s = self.s.borrow_mut();
            s.receipt_sequence += 1;
            book.as_object_mut().unwrap().insert("productionAvailableAtMs".into(), json!(self.base.now_ms()));
            book.as_object_mut().unwrap().insert("productionSequence".into(), json!(s.receipt_sequence));
        }
        let owned = self
            .base
            .inner
            .borrow()
            .state
            .pointer(&format!("/owned/{}", book.get("exchangeId").and_then(V::as_str).unwrap_or("")))
            .cloned();
        if self.base.inner.borrow().reconciliation_risk_only && owned.is_none() {
            return Ok(());
        }
        if owned.is_none() && !entry_window_gate(&self.base.inner, &self.base.activation, self.base.now_ms()) {
            let reason = self.base.inner.borrow().reason.clone().unwrap_or_default();
            self.base.entry_decision(&book, "WINDOW_BLOCKED", &json!({"detailCode": reason}));
            return Ok(());
        }
        let evaluated = self.evaluate(&book, None)?;
        let d = evaluated.get("decision").unwrap().clone();
        let proof = evaluated.get("proof").unwrap().clone();
        self.base.inner.borrow_mut().status = "monitoring".into();
        self.base.inner.borrow_mut().reason = d.get("reason").and_then(V::as_str).map(str::to_string);
        let exchange_id = book.get("exchangeId").and_then(V::as_str).unwrap_or("").to_string();
        if !d.get("riskExit").is_none_or(V::is_null)
            && self
                .base
                .inner
                .borrow()
                .state
                .pointer(&format!("/control/exits/{exchange_id}"))
                .is_none()
        {
            let obligation = json!({
                "marketId": market_id,
                "reason": d.get("riskExit").unwrap().clone(),
                "triggeredAt": iso(self.base.now_ms()),
                "triggerBid": book.pointer("/bids/0/price").unwrap().clone(),
                "sourceVersion": book.get("versionKey").unwrap().clone(),
            });
            let exit_id = exchange_id.clone();
            self.base.update_control(
                move |c| {
                    c.pointer_mut("/exits").unwrap().as_object_mut().unwrap().insert(exit_id.clone(), obligation);
                },
                &json!({"type": "exit_obligation_latched", "exchangeId": exchange_id, "marketId": market_id}),
            )?;
        }
        if !(["candidate_buy", "reduce"].contains(&d.get("action").and_then(V::as_str).unwrap_or(""))
            && d.get("quantity").and_then(V::as_i64).unwrap_or(0) >= 1)
        {
            self.publish();
            return Ok(());
        }
        let action = if d.get("action").and_then(V::as_str) == Some("candidate_buy") {
            "buy"
        } else {
            "sell"
        };
        let price = if action == "buy" || !self.s.borrow().bank.get("receiptPolicy").is_none_or(V::is_null) {
            d.get("hardLimitWithCushion").unwrap().clone()
        } else {
            book.pointer("/bids/0/price").unwrap().clone()
        };
        let quantity = (d.get("quantity").and_then(V::as_i64).unwrap_or(0)).min(
            (units(self.base.activation.get("maxOrderCost").unwrap_or(&V::Null))
                / units(&price)) as i64,
        );
        if quantity < 1 {
            return Ok(());
        }
        let spec = json!({
            "intentId": format!("intent-{}", uuid::Uuid::new_v4()),
            "idempotencyKey": uuid::Uuid::new_v4().to_string(),
            "marketId": market_id,
            "exchangeId": book.get("exchangeId").unwrap().clone(),
            "side": "yes",
            "action": action,
            "quantity": quantity,
            "price": price,
            "expirationDate": iso(self.base.now_ms() + 30000),
            "attribution": {
                "runId": self.base.activation.get("runId").unwrap().clone(),
                "strategyId": self.base.activation.get("strategyId").unwrap().clone(),
                "modelHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
                "signalId": proof.get("outputHash").unwrap().clone(),
                "reason": d.get("reason").unwrap().clone(),
            },
        });
        let snapshot = self.base.snapshot(Some(&json!({
            "tournamentId": crate::TOURNAMENT_ID,
            "marketId": market_id,
            "exchangeId": book.get("exchangeId").unwrap().clone(),
            "at": book.get("receivedAt").unwrap().clone(),
            "requestStartedAt": book.get("requestStartedAt").unwrap().clone(),
            "bids": book.get("bids").unwrap().clone(),
            "asks": book.get("asks").unwrap().clone(),
        })));
        let r = prepare_intent(&self.base.inner.borrow().state, &spec, &snapshot, &json!(self.base.now_ms()));
        if r.get("ok") != Some(&V::Bool(true)) {
            self.base.inner.borrow_mut().reason = r.get("code").and_then(V::as_str).map(str::to_string);
            self.publish();
            return Ok(());
        }
        let mut reference_proof = proof;
        reference_proof
            .as_object_mut()
            .unwrap()
            .insert("settlementBook".into(), book.clone());
        self.base.ledger.borrow_mut().prepare_intent(&json!({
            "method": "POST",
            "path": "/orders",
            "intentId": spec.get("intentId").unwrap().clone(),
            "idempotencyKey": spec.get("idempotencyKey").unwrap().clone(),
            "body": r.pointer("/intent/body").unwrap().clone(),
            "bodyJson": r.pointer("/intent/bodyJson").unwrap().clone(),
            "payloadHash": r.pointer("/intent/payloadHash").unwrap().clone(),
            "attribution": {
                "ownerId": "Codex",
                "runId": self.base.activation.get("runId").unwrap().clone(),
                "strategyId": spec.pointer("/attribution/strategyId").unwrap().clone(),
                "modelHash": spec.pointer("/attribution/modelHash").unwrap().clone(),
                "marketId": market_id,
                "exchangeId": book.get("exchangeId").unwrap().clone(),
            },
        })).map_err(|e| format!("{NEED}LEDGER:{e}"))?;
        self.base.commit(&r, &json!({"type": "intent_prepared", "intentId": spec.get("intentId").unwrap().clone(), "reason": d.get("reason").unwrap().clone(), "referenceProof": reference_proof}))?;
        self.submit(r.get("intent").unwrap(), Some(&snapshot), false, Some(&reference_proof))
    }
    pub fn publish(&self) {
        self.base.publish();
        let s = self.s.borrow();
        let file = self.runtime_dir.join("competition-stats.json");
        let Ok(raw) = std::fs::read_to_string(&file) else { return };
        let Ok(mut stats) = serde_json::from_str::<V>(&raw) else { return };
        {
            let (status_v, reason_v) = (
                stats.get("status").and_then(V::as_str).unwrap_or("").to_string(),
                stats.get("reason").and_then(V::as_str).unwrap_or("").to_string(),
            );
            let o = stats.as_object_mut().unwrap();
            o.insert("strategyId".into(), self.base.activation.get("strategyId").unwrap().clone());
            o.insert(
                "strategyBankHash".into(),
                self.base.activation.get("strategyBankHash").unwrap().clone(),
            );
            o.insert("latestNative".into(), s.latest_native.clone());
            o.insert("eligibleResearchMarkets".into(), json!(self.eligible_market_ids()));
            o.insert(
                "lastFreshnessRepair".into(),
                if s.last_freshness_repair.is_null() { V::Null } else { s.last_freshness_repair.clone() },
            );
            let extra = crate::gates::production_entry_state(
                &self.base.activation,
                self.base.now_ms() as f64,
                &self.eligible_market_ids(),
                self.base.inner.borrow().state.get("owned").unwrap_or(&V::Null),
                self.base.active().as_ref(),
                &status_v,
                &reason_v,
            );
            if let Some(eo) = extra.as_object() {
                for (k, v) in eo {
                    o.insert(k.clone(), v.clone());
                }
            }
        }
        // Native-decision stats replace the file via tmp+rename mode 0o600.
        let tmp = file.with_extension("json.native.tmp");
        if let Ok(pretty) = serde_json::to_string_pretty(&stats) {
            let _ = std::fs::write(&tmp, pretty);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.base.close()
    }
}

// Settlement entryReferenceAdmitted — re-evaluates the pinned native decision
// inside the dispatch fence.
fn settlement_reference_gate(
    s: &Rc<RefCell<SettlementState>>,
    inner: &Rc<RefCell<crate::controller::ControllerInner>>,
    runtime_dir: &Path,
    activation: &V,
    i: &V,
    permit: &V,
    now_ms: i64,
) -> bool {
    let current = inner.borrow().current_permit.clone().unwrap_or(V::Null);
    if permit.get("retry") == Some(&V::Bool(true))
        && current.get("retry") == Some(&V::Bool(true))
        && i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
        && now_ms >= instant(i.get("expirationDate").unwrap_or(&V::Null))
    {
        return true;
    }
    if i.get("action").and_then(V::as_str) == Some("buy")
        && !entry_window_gate(inner, activation, now_ms)
    {
        return false;
    }
    let ok = (|| -> Result<bool, String> {
        let proof = current.get("referenceProof").cloned().unwrap_or(V::Null);
        let book = current.get("settlementBook").cloned().unwrap_or(V::Null);
        need(
            !book.is_null()
                && proof.get("bankHash") == activation.get("strategyBankHash")
                && proof.get("bookHash").and_then(V::as_str) == Some(digest(&book).as_str()),
            "PROOF",
        )?;
        // Re-run the native decision against the admitted settlement book.
        let dir = runtime_dir.join("native-decisions").join(uuid::Uuid::new_v4().to_string());
        let bank = s.borrow().bank.clone();
        let input = settlement_gate_input(s, inner, activation, &book, i)?;
        let checked = run_settlement_value(&input, &dir, &s.borrow().executable)?;
        need(
            checked.get("binaryHash").and_then(V::as_str)
                == bank.pointer("/nativeBackend/binaryHash").and_then(V::as_str)
                && checked.pointer("/result/engineHash").and_then(V::as_str)
                    == bank.pointer("/nativeBackend/engineHash").and_then(V::as_str),
            "NATIVE_IDENTITY",
        )?;
        let d = checked
            .pointer("/result/decisions")
            .and_then(V::as_array)
            .and_then(|a| {
                a.iter()
                    .find(|c| c.get("exchangeId") == book.get("exchangeId"))
                    .cloned()
            })
            .unwrap_or(V::Null);
        need(!d.is_null(), "DECISION_SCOPE")?;
        let want = if i.get("action").and_then(V::as_str) == Some("buy") {
            "candidate_buy"
        } else {
            "reduce"
        };
        Ok(
            d.get("action").and_then(V::as_str) == Some(want)
                && d.get("quantity").and_then(V::as_i64).unwrap_or(0) >= i.get("quantity").and_then(V::as_i64).unwrap_or(0)
                && !(i.get("action").and_then(V::as_str) == Some("buy")
                    && units(i.get("price").unwrap_or(&V::Null)) > units(d.get("hardLimitWithCushion").unwrap_or(&V::Null)))
                && !(i.get("action").and_then(V::as_str) == Some("sell")
                    && !bank.get("receiptPolicy").is_none_or(V::is_null)
                    && units(i.get("price").unwrap_or(&V::Null)) < units(d.get("hardLimitWithCushion").unwrap_or(&V::Null))),
        )
    })();
    ok.unwrap_or(false)
}

// nativeInput equivalent for the fence: the intent is skipped so it is not
// double-counted against itself.
fn settlement_gate_input(
    s: &Rc<RefCell<SettlementState>>,
    inner: &Rc<RefCell<crate::controller::ControllerInner>>,
    activation: &V,
    book: &V,
    skip: &V,
) -> Result<V, String> {
    let state = inner.borrow().state.clone();
    let intents: Vec<V> = state
        .get("intents")
        .and_then(V::as_object)
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    let skip_id = skip.get("intentId").and_then(V::as_str).unwrap_or("");
    let now = instant(&json!(iso(0)));
    let _ = now;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut positions = Vec::new();
    for (exchange_id, p) in state.get("owned").and_then(V::as_object).cloned().unwrap_or_default() {
        let entry = intents
            .iter()
            .filter(|i| {
                i.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str())
                    && i.get("action").and_then(V::as_str) == Some("buy")
                    && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
            })
            .last();
        need(entry.is_some(), "OWNED_SOURCE")?;
        let entry = entry.unwrap();
        positions.push(json!({
            "candidateId": format!("{}:YES", p.get("marketId").and_then(V::as_str).unwrap_or("")),
            "quantity": p.get("quantity").unwrap().clone(),
            "cost": p.get("cost").unwrap().clone(),
            "enteredAtMs": instant(
                entry
                    .pointer("/reconciledFills/0/filledAt")
                    .or_else(|| entry.get("preparedAt"))
                    .unwrap_or(&V::Null),
            ),
        }));
    }
    let market_id = book.get("marketId").and_then(V::as_str).unwrap_or("");
    let contract = s.borrow().checked_contracts.get(market_id).cloned().unwrap_or(V::Null);
    need(
        !contract.is_null()
            && contract.get("rootHash")
                == s.borrow().bank.pointer(&format!("/contracts/{market_id}/rootHash")).cloned().as_ref()
            && instant(contract.get("at").unwrap_or(&V::Null)) <= instant(book.get("requestStartedAt").unwrap())
            && now - instant(contract.get("at").unwrap()) <= 180000,
        "CONTRACT_NOT_CURRENT",
    )?;
    let cumulative: i128 = intents
        .iter()
        .filter(|i| {
            i.get("intentId").and_then(V::as_str) != Some(skip_id)
                && i.get("action").and_then(V::as_str) == Some("buy")
                && i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
        })
        .map(|i| units(i.get("price").unwrap_or(&V::Null)) * i.get("quantity").and_then(V::as_i64).unwrap_or(0) as i128)
        .sum();
    let reservations: Vec<V> = intents
        .iter()
        .filter(|i| {
            i.get("intentId").and_then(V::as_str) != Some(skip_id)
                && i.get("action").and_then(V::as_str) == Some("buy")
                && i.get("status").and_then(V::as_str) == Some("prepared")
        })
        .map(|i| {
            json!({
                "candidateId": format!("{}:YES", i.get("marketId").and_then(V::as_str).unwrap_or("")),
                "maximumCost": crate::core::format_micros(
                    units(i.get("price").unwrap_or(&V::Null))
                        * i.get("quantity").and_then(V::as_i64).unwrap_or(0) as i128,
                ),
            })
        })
        .collect();
    let account = inner.borrow().account.clone().unwrap_or(V::Null);
    let positions_at = instant(account.pointer("/positions/at").unwrap_or(&V::Null));
    let orders_at = instant(account.pointer("/orders/at").unwrap_or(&V::Null));
    let known_at_ms = positions_at.max(orders_at);
    let position_rows = account.pointer("/positions/rows").and_then(V::as_array).cloned().unwrap_or_default();
    let order_rows = account.pointer("/orders/rows").and_then(V::as_array).cloned().unwrap_or_default();
    let candidates: Vec<V> = s
        .borrow()
        .bank
        .get("candidates")
        .and_then(V::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let mut c = c;
            let has_book = c.get("exchangeId") == book.get("exchangeId");
            let book_v = if has_book {
                json!({
                    "requestStartedAtMs": instant(book.get("requestStartedAt").unwrap_or(&V::Null)),
                    "receivedAtMs": instant(book.get("receivedAt").unwrap_or(&V::Null)),
                    "availableAtMs": book.get("productionAvailableAtMs").unwrap().clone(),
                    "sequence": book.get("productionSequence").unwrap().clone(),
                    "sourceHash": digest(book),
                    "representation": "direct_yes",
                    "bids": book.get("bids").unwrap().clone(),
                    "asks": book.get("asks").unwrap().clone(),
                })
            } else {
                V::Null
            };
            let exchange_id = c.get("exchangeId").and_then(V::as_str).unwrap_or("").to_string();
            c.as_object_mut().unwrap().insert("book".into(), book_v);
            c.as_object_mut().unwrap().insert(
                "account".into(),
                json!({
                    "positionsAtMs": positions_at,
                    "ordersAtMs": orders_at,
                    "knownAtMs": known_at_ms,
                    "complete": account.pointer("/positions/complete") == Some(&V::Bool(true))
                        && account.pointer("/orders/complete") == Some(&V::Bool(true)),
                    "positionExcluded": position_rows.iter().any(|p| {
                        p.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str())
                            && p.get("quantity").and_then(V::as_i64).unwrap_or(0) != 0
                    }),
                    "openOrderExcluded": order_rows.iter().any(|o| {
                        o.get("exchangeId").and_then(V::as_str) == Some(exchange_id.as_str())
                    }),
                }),
            );
            c
        })
        .collect();
    let cash = inner.borrow().cash.clone().unwrap_or(V::Null);
    let mut input = json!({
        "schemaVersion": 1,
        "mode": "settlement_value_policy_v1",
        "asOfMs": now,
        "policy": s.borrow().bank.get("policy").unwrap().clone(),
        "wallet": {
            "capital": activation.get("sizingCapital").unwrap().clone(),
            "cash": crate::core::format_micros(
                conservative_cash_floor(cash.pointer("/data/myBalance").unwrap_or(&V::Null))?,
            ),
            "cumulativeBuys": crate::core::format_micros(cumulative),
            "positions": positions,
            "reservations": reservations,
        },
        "candidates": candidates,
    });
    if s.borrow().bank.get("mode").and_then(V::as_str) == Some("baal_probability_receipt_production_v1") {
        input.as_object_mut().unwrap().insert("schemaVersion".into(), json!(2));
        input.as_object_mut().unwrap().insert("mode".into(), json!("probability_receipt_policy_v1"));
        input
            .as_object_mut()
            .unwrap()
            .insert("receiptPolicy".into(), s.borrow().bank.get("receiptPolicy").unwrap().clone());
        let mut last_exited: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for i in &intents {
            if i.get("action").and_then(V::as_str) == Some("sell")
                && i.get("status").and_then(V::as_str) == Some("closed")
                && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
                && state
                    .pointer(&format!("/owned/{}", i.get("exchangeId").and_then(V::as_str).unwrap_or("")))
                    .is_none()
            {
                let id = format!("{}:YES", i.get("marketId").and_then(V::as_str).unwrap_or(""));
                let at = instant(i.get("closedAt").unwrap_or(&V::Null));
                last_exited.entry(id).and_modify(|e| *e = (*e).max(at)).or_insert(at);
            }
        }
        let risk_exits: serde_json::Map<String, V> = state
            .pointer("/control/exits")
            .and_then(V::as_object)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(_eid, x)| {
                Some((
                    format!("{}:YES", x.get("marketId").and_then(V::as_str)?),
                    x.get("reason").unwrap().clone(),
                ))
            })
            .collect();
        input.as_object_mut().unwrap().insert(
            "receiptState".into(),
            json!({"riskExits": risk_exits, "lastExitedAtMs": last_exited}),
        );
    }
    Ok(input)
}
