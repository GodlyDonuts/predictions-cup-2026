//! Package inputs pinned by the existing immutable production control.
//! Loading a document never approves its settlement certificate.
use crate::{authority::Activation,feedback::Result,relationship_strategy::{self,Policy,SettlementCertificate}};
use baal_transport_service::{Scope,Receipt,decimal_units};
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,fs,io::Read,path::PathBuf};
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Settings{
 pub certificate_bundle:PathBuf,pub bundle_sha256:String,
 pub trusted_certificate_sha256:BTreeSet<String>,pub refresh_ms:u64,
 pub maximum_relationship_age_ms:u64,pub maximum_rule_age_ms:u64,
 pub buffer_per_share:String,pub minimum_surplus:String,
}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Bundle{pub certificates:Vec<SettlementCertificate>,pub original_receipts:Vec<Receipt>}
impl Settings{
 pub fn validate(&self)->Result<()>{
  need(self.certificate_bundle.is_absolute()&&hex(&self.bundle_sha256)&&self.trusted_certificate_sha256.iter().all(|s|hex(s)),"PACKAGE_SETTINGS_SOURCE")?;
  need((5_000..=300_000).contains(&self.refresh_ms)&&(10_000..=300_000).contains(&self.maximum_relationship_age_ms)&&(10_000..=86_400_000).contains(&self.maximum_rule_age_ms),"PACKAGE_SETTINGS_CLOCK")?;
  need(decimal_units(&self.buffer_per_share)?>=0&&decimal_units(&self.minimum_surplus)?>0,"PACKAGE_SETTINGS_MARGIN")
 }
 pub fn load(&self,scope:&Scope)->Result<Bundle>{
  self.validate()?;let m=fs::symlink_metadata(&self.certificate_bundle)?;need(m.is_file()&&!m.file_type().is_symlink()&&m.len()<=8*1024*1024,"PACKAGE_SETTINGS_FILE")?;
  let mut data=Vec::new();fs::File::open(&self.certificate_bundle)?.take(8*1024*1024+1).read_to_end(&mut data)?;
  need(data.len()<=8*1024*1024&&format!("{:x}",Sha256::digest(&data))==self.bundle_sha256,"PACKAGE_SETTINGS_SOURCE_CHANGED")?;
  let b:Bundle=serde_json::from_slice(&data)?;need(b.certificates.len()<=1000&&b.original_receipts.len()<=10_000,"PACKAGE_SETTINGS_BOUND")?;
  let mut seen=BTreeSet::new();
  for c in &b.certificates{let digest=relationship_strategy::certificate_sha256(c)?;need(seen.insert(digest.clone()),"PACKAGE_SETTINGS_DUPLICATE_CERTIFICATE")?;
   if self.trusted_certificate_sha256.contains(&digest){need(c.tournament_id==scope.tournament_id&&(2..=3).contains(&c.contracts.len()),"PACKAGE_SETTINGS_CERTIFICATE_SCOPE")?;let mut exchanges=BTreeSet::new();for r in &c.contracts{need(scope.exchanges.get(&r.exchange_id)==Some(&r.market_id)&&exchanges.insert(&r.exchange_id),"PACKAGE_SETTINGS_CERTIFICATE_SCOPE")?;}}
  }
  need(self.trusted_certificate_sha256.is_subset(&seen),"PACKAGE_SETTINGS_APPROVED_CERTIFICATE_MISSING")?;Ok(b)
 }
 /// Per-component quantity respects the ORIGINAL aggregate activation cap.
 /// Each certificate is evaluated separately so a three-leg cap does not shrink
 /// a two-leg opportunity or cause an oversized best plan to hide smaller ones.
 pub fn policy(&self,a:&Activation,at:u64,legs:usize)->Result<Policy>{
  self.validate()?;need((2..=3).contains(&legs),"PACKAGE_SETTINGS_LEGS")?;let maximum=a.maximum_quantity as u64/legs as u64;need(maximum>0,"PACKAGE_SETTINGS_QUANTITY")?;
  let mut quantities=BTreeSet::new();let mut decade=1u64;
  loop{for n in [1u64,2,5]{if let Some(q)=decade.checked_mul(n){if q<=maximum{quantities.insert(q);}}}if decade>maximum/10{break}decade=decade.checked_mul(10).ok_or("PACKAGE_SETTINGS_QUANTITY")?;}
  quantities.insert(maximum);
  Ok(Policy{now_ms:at,max_relationship_age_ms:self.maximum_relationship_age_ms,max_rule_age_ms:self.maximum_rule_age_ms,max_book_age_ms:10_000,max_book_skew_ms:5_000,entry_fee_per_share_per_leg:0,buffer_per_share:decimal_units(&self.buffer_per_share)?,minimum_surplus:decimal_units(&self.minimum_surplus)?,maximum_quantity:maximum,quantities:quantities.into_iter().collect(),maximum_retained_candidates:32})
 }
 pub fn approved<'a>(&self,b:&'a Bundle,at:u64)->Result<Vec<&'a SettlementCertificate>>{
  let mut out=Vec::new();for c in &b.certificates{if self.trusted_certificate_sha256.contains(&relationship_strategy::certificate_sha256(c)?)&&c.issued_at_ms<=at&&at<c.expires_at_ms{out.push(c)}}Ok(out)
 }
}
