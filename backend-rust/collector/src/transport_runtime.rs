// live-paper/transport-runtime.mjs — serialized read/write client, hash-chain
// Journal and helpers live in baal-service-runtime (the transport-runtime
// variant: writeInterval + schema v3 readOnly + monotonic timing events).
pub use baal_service_runtime::atomic::atomic_json;
pub use baal_service_runtime::client::{ReadClient, ReadClientOptions};
pub use baal_service_runtime::exclusions::exclusion_set;
pub use baal_service_runtime::journal::Journal;
pub use crate::paper_runtime::{lock, PaperLock};
