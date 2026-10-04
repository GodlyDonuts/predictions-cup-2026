// Included inside the engine module so frozen engine.rs needs no access changes.
// This const inspector cannot modify cash, intents, positions or fills.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReadIntent {
    instrument: String,
    market_id: String,
    intent_id: String,
    action: &'static str,
    quantity: i64,
    limit: String,
    decision_at: String,
    minimum_request_started_at: String,
    expires_at: String,
    decision_sequence: String,
    decision_source_ns: String,
    reason: &'static str,
}
impl Wallet {
    pub fn pending_read_intents(&self, c: &Config) -> Result<Vec<PendingReadIntent>, String> {
        let mut rows = Vec::new();
        for (instrument, p) in &self.pending {
            let b = self.books.get(instrument).ok_or("PENDING_READ_BOOK_MISSING")?;
            let audit = p.audit.as_ref().ok_or("PENDING_READ_AUDIT_MISSING")?;
            if b.instrument != *instrument || audit.decision_at != p.at || p.quantity < 1 {
                return Err("PENDING_READ_IDENTITY_INVALID".into());
            }
            let earliest = p.at.checked_add(c.latency_ms).ok_or("PENDING_READ_CLOCK_OVERFLOW")?;
            let expiry = c.cycle_interval_ms.checked_mul(4)
                .and_then(|n| n.checked_add(c.max_data_age_ms))
                .and_then(|n| n.checked_add(p.at)).ok_or("PENDING_READ_CLOCK_OVERFLOW")?;
            rows.push(PendingReadIntent {
                instrument: instrument.clone(), market_id: b.market_id.clone(),
                intent_id: p.id.to_string(), action: if p.buy { "buy" } else { "sell" },
                quantity: p.quantity, limit: money(p.limit), decision_at: iso(p.at),
                minimum_request_started_at: iso(earliest), expires_at: iso(expiry),
                decision_sequence: p.decision_version.0.to_string(),
                decision_source_ns: p.decision_version.1.to_string(), reason: audit.reason,
            });
        }
        rows.sort_by(|a,b| a.instrument.cmp(&b.instrument));
        Ok(rows)
    }
}
