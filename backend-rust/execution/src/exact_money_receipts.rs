// execution/exact-money-receipts.mjs — exact-decimal receipts bound to raw API
// bodies. Already ported in this crate as receipts.rs (a prior batch used the
// module's own naming); re-export so the one-file-per-module mapping resolves
// to a single implementation rather than a divergent copy.
pub use crate::receipts::{exact_money_fetch, parse_number_lexemes, MoneyReceiptOptions, MoneyReceiptWriter, BASE};
