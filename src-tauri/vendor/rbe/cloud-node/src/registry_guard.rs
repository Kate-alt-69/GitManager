//! Always-available guard for interrupted Cloud Node registry replacements.

use crate::CloudNodeStore;

pub(crate) const REGISTRY_INGEST_PENDING: &str = ".registry-ingest.pending";

/// Refuse to publish or negotiate a Cloud Node snapshot while a previous
/// registry replacement is known to have stopped before its commit point.
pub(crate) fn ensure_registry_ingest_committed(store: &CloudNodeStore) -> anyhow::Result<()> {
    let marker = store.summary().root.join(REGISTRY_INGEST_PENDING);
    if marker.exists() {
        anyhow::bail!(
            "Cloud Node registry replacement is incomplete at {}; run ingest-registry or sync-registry again with a complete trusted export before synchronization",
            marker.display()
        );
    }
    Ok(())
}
