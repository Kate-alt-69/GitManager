//! Low-level RBE Cloud Node primitives.
//!
//! Cloud Node sits below REL/RELC. Language programs never receive node keys,
//! tunnel identities, topology, storage roots, or direct access to this crate.

#[cfg(feature = "peer-auth")]
mod auth;
#[cfg(feature = "peer-client")]
mod client;
mod config;
#[cfg(feature = "peer-auth")]
mod crypto;
mod durable;
mod format;
#[cfg(feature = "journal-ingest")]
mod ingest;
#[cfg(feature = "local-sync")]
mod local_sync;
#[cfg(any(feature = "peer-client", feature = "provider-sync"))]
mod outbound_cache;
mod protocol;
#[cfg(feature = "provider-sync")]
mod provider;
#[cfg(feature = "provider-sync")]
mod provider_diagnostics;
#[cfg(feature = "provider-sync")]
#[path = "provider_sync_checked.rs"]
mod provider_sync;
#[cfg(feature = "provider-sync")]
#[path = "provider_sync_guard.rs"]
mod provider_sync_guard;
#[cfg(feature = "provider-sync")]
#[path = "provider_sync.rs"]
mod provider_sync_raw;
#[cfg(feature = "recovery")]
mod recovery;
#[cfg(feature = "registry-ingest")]
mod registry_ingest;
mod registry_guard;
#[cfg(feature = "server")]
mod server;
mod settings_path;
mod store;
mod sync;
mod transfer;

#[cfg(feature = "peer-auth")]
pub use auth::{random_session_and_nonce, NodeProof, NodeProofKind, DEFAULT_AUTH_SKEW_MS};
#[cfg(feature = "peer-client")]
pub use client::{
    negotiate_sync, probe_upstream, synchronize_upstream, AuthenticatedPeer, SyncNegotiation,
    KNOCK_PATH, SESSION_PROOF_HEADER, SYNC_PATH, TRANSFER_PATH,
};
#[cfg(not(feature = "peer-client"))]
pub const KNOCK_PATH: &str = "/.rbe/cn/v1/knock";
#[cfg(not(feature = "peer-client"))]
pub const SYNC_PATH: &str = "/.rbe/cn/v1/sync";
#[cfg(not(feature = "peer-client"))]
pub const TRANSFER_PATH: &str = "/.rbe/cn/v1/transfer";
#[cfg(not(feature = "peer-client"))]
pub const SESSION_PROOF_HEADER: &str = "x-rbe-cn-proof";
pub use config::{
    CloudNodeSettings, NodeMode, NodeSettings, ProviderAuthMode, ProviderAuthSettings,
    ProviderConflictPolicy, ProviderKind, ProviderSettings, ReplicationSettings, ReplicationTarget,
    UpstreamSettings,
};
#[cfg(feature = "peer-auth")]
pub use crypto::{
    load_signing_key_from_env, public_key_hex, sign_challenge, verify_challenge,
    CLOUD_NODE_PRIVATE_KEY_ENV,
};
pub use format::{
    BlobKind, BlobManifest, ByteRangeChange, ChunkRef, FolderEntry, BLOB_FORMAT_VERSION,
};
#[cfg(feature = "local-sync")]
pub use local_sync::{
    load_local_sync_settings, LocalDirectoryRestoreResult, LocalDirectoryStatus,
    LocalDirectorySyncResult, LocalSyncSettings,
};
pub use secret_paths::is_default_secret_path;
mod secret_paths;
pub use protocol::{Frame, FrameKind, CN_PROTOCOL, MAX_FRAME_BYTES};
#[cfg(feature = "provider-sync")]
pub use provider::ProviderClient;
#[cfg(feature = "provider-sync")]
pub use provider_sync::{
    provider_status, synchronize_provider, ProviderSyncAction, ProviderSyncRelation,
    ProviderSyncResult, ProviderSyncStatus,
};
#[cfg(feature = "recovery")]
pub use recovery::{CloudNodeRecoveryReceiver, RecoveryReceipt};
#[cfg(feature = "registry-ingest")]
pub use registry_ingest::{ingest_registry_export, RegistryIngestResult, RegistryStoredObject};
#[cfg(feature = "registry-ingest")]
pub use registry_ingest::{synchronize_registry_export, RegistrySyncResult};
#[cfg(feature = "server")]
pub use server::{
    AcceptedKnock, AuthenticatedSession, CloudNodeAuthenticator, DEFAULT_SESSION_TTL_MS,
    MAX_AUTH_PROOF_BYTES,
};
pub use settings_path::{
    discover_settings_path, LEGACY_SETTINGS_FILE_NAME, PREFERRED_SETTINGS_FILE_NAME,
    PREFERRED_SETTINGS_FILE_NAME as SETTINGS_FILE_NAME,
};
pub use store::{CloudNodeStore, StoreSummary, StoredObject};
pub use sync::{SyncObject, SyncPlan, SyncPlanHeader};
pub use transfer::{TransferChunk, TransferResource, MAX_TRANSFER_DATA_BYTES, RESUME_ACK_HEADER};
