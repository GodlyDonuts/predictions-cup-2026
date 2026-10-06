// live-paper/training-horizons.mjs
pub const DEFAULT_HORIZONS: [i64; 4] = [60, 180, 300, 600];
pub fn training_horizons(value: &[i64]) -> Result<Vec<i64>, String> {
    let unique: std::collections::BTreeSet<i64> = value.iter().copied().collect();
    if value.is_empty() || value.len() > 8 || unique.len() != value.len()
        || value.iter().any(|x| !(60..=86400).contains(x)) {
        return Err("Invalid training horizons".into());
    }
    Ok(value.to_vec())
}
pub fn parse_training_horizons(text: &str) -> Result<Vec<i64>, String> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || b == b',') {
        return Err("Invalid training horizons".into());
    }
    let parts: Result<Vec<i64>, _> = text.split(',').map(|s| s.parse::<i64>()).collect();
    training_horizons(&parts.map_err(|_| "Invalid training horizons".to_string())?)
}
