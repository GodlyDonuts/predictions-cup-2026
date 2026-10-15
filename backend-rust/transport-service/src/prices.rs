//! The existing Baal canonical tick protocol: exact IEEE tick representations.
//! This is for per-share prices only. Cash and accounting amounts stay decimal.
use crate::{Result,decimal_units,fail};
use serde_json::Value;
pub fn official_tick_units(v:&Value)->Result<i128>{
 if let Some(text)=v.as_str(){let p=decimal_units(text)?;if !(0..=1_000_000).contains(&p)||p%5000!=0{return Err(fail("OFFICIAL_PRICE_OFF_TICK"))}return Ok(p)}
 let n=v.as_f64().filter(|n|n.is_finite()&&n.to_bits()!=(-0.0f64).to_bits()).ok_or_else(||fail("OFFICIAL_PRICE_REPRESENTATION"))?;
 for tick in 0..=200 {let units=tick*5000i128;let text=format!("{}.{:06}",units/1_000_000,units%1_000_000);let other=1_000_000-units;let opposite=format!("{}.{:06}",other/1_000_000,other%1_000_000);let canonical=text.parse::<f64>().unwrap();let complement=opposite.parse::<f64>().unwrap();if [canonical,1.0-complement,1.0-(1.0-canonical)].iter().any(|x|x.to_bits()==n.to_bits()){return Ok(units)}}
 Err(fail("OFFICIAL_PRICE_REPRESENTATION"))
}
#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 #[test]fn binary_no_complement_is_not_a_general_rounding_tolerance(){assert_eq!(official_tick_units(&json!(0.06000000000000005)).unwrap(),60000);assert_eq!(official_tick_units(&json!("0.060000")).unwrap(),60000);for v in [json!(0.0600000001),json!(0.061),json!(-0.0),json!("0.06000000000000005"),json!(1.005)]{assert!(official_tick_units(&v).is_err());}}
 #[test]fn all_documented_ticks_and_binary_complements_map_uniquely(){for tick in 0..=200{let p=tick*5000i128;let n=format!("{}.{:06}",p/1_000_000,p%1_000_000).parse::<f64>().unwrap();let other=1_000_000-p;let no=format!("{}.{:06}",other/1_000_000,other%1_000_000).parse::<f64>().unwrap();for x in [n,1.0-no,1.0-(1.0-n)]{assert_eq!(official_tick_units(&json!(x)).unwrap(),p);}}}
}
