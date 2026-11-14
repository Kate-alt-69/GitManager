//! Durable outbound snapshot cache shared by the RBE peer client and object-provider sync.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::durable;
use crate::format::BlobKind;
use crate::store::CloudNodeStore;
use crate::sync::{SyncObject, SyncPlan, SyncPlanHeader};
use crate::transfer::TransferResource;

pub(crate) async fn prepare_outbound_cache(
    store: &CloudNodeStore,
    peer_node_id: &str,
    plan: &SyncPlan,
) -> anyhow::Result<PathBuf> {
    let header = plan.header()?;
    let peer_root = outbound_peer_cache_root(store, peer_node_id);
    durable::create_dir_all_async(&peer_root).await?;
    let snapshot_name = hex::encode(header.root_sha256);
    prune_outbound_cache_snapshots(&peer_root, &snapshot_name).await?;
    let cache_root = peer_root.join(&snapshot_name);
    durable::create_dir_all_async(&cache_root).await?;

    for object in plan.ordered() {
        let manifest_hash = sha256_path(&object.manifest_path).await?;
        let manifest_target = cached_resource_path(
            &cache_root,
            object,
            TransferResource::Manifest,
            &object.manifest_path,
        )?;
        materialize_cached_resource(&object.manifest_path, &manifest_target, manifest_hash).await?;

        match object.kind {
            BlobKind::Folder => {}
            BlobKind::Video => {
                for chunk_path in &object.chunk_paths {
                    let chunk_hash = sha256_path(chunk_path).await?;
                    let chunk_target = cached_resource_path(
                        &cache_root,
                        object,
                        TransferResource::VideoChunk,
                        chunk_path,
                    )?;
                    materialize_cached_resource(chunk_path, &chunk_target, chunk_hash).await?;
                }
            }
            BlobKind::File => {
                let payload = object.payload_path.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cloud Node file {} is missing its generation payload",
                        object.logical_path
                    )
                })?;
                let payload_target = cached_resource_path(
                    &cache_root,
                    object,
                    TransferResource::FilePayload,
                    payload,
                )?;
                materialize_cached_resource(payload, &payload_target, object.content_sha256)
                    .await?;
            }
        }
    }

    // Detect a LOCAL write racing the cache freeze. Partial cache bytes stay
    // durable, but they are never marked ready or uploaded as a mixed root.
    if store.sync_plan()?.header()? != header {
        anyhow::bail!(
            "Cloud Node local snapshot changed while preparing outbound cache; retry required"
        );
    }
    write_cache_ready_marker(&cache_root, header).await?;
    Ok(cache_root)
}

pub(crate) fn outbound_peer_cache_root(store: &CloudNodeStore, peer_node_id: &str) -> PathBuf {
    store
        .summary()
        .root
        .join(".cache")
        .join("outbound")
        .join(peer_node_id)
}

pub(crate) fn cached_resource_path(
    cache_root: &Path,
    object: &SyncObject,
    resource: TransferResource,
    source: &Path,
) -> anyhow::Result<PathBuf> {
    let object_root = cache_root.join(hex::encode(object.object_key));
    match resource {
        TransferResource::Manifest => Ok(object_root.join("manifest.blob.cn")),
        TransferResource::FilePayload => Ok(object_root.join("payload")),
        TransferResource::VideoChunk => {
            let name = source.file_name().ok_or_else(|| {
                anyhow::anyhow!(
                    "Cloud Node video chunk has no cacheable file name: {}",
                    source.display()
                )
            })?;
            Ok(object_root.join("chunks").join(name))
        }
    }
}

async fn materialize_cached_resource(
    source: &Path,
    target: &Path,
    expected_sha256: [u8; 32],
) -> anyhow::Result<()> {
    let source_size = tokio::fs::metadata(source).await?.len();
    if cached_file_matches(target, expected_sha256, source_size).await? {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        durable::create_dir_all_async(parent).await?;
    }
    if target.exists() {
        durable::remove_file_async(target).await?;
    }

    let part = cache_part_path(target)?;
    if part.exists() {
        durable::remove_file_async(&part).await?;
    }
    tokio::fs::copy(source, &part).await?;
    tokio::fs::OpenOptions::new().write(true).open(&part).await?.sync_all().await?;
    if !cached_file_matches(&part, expected_sha256, source_size).await? {
        let _ = durable::remove_file_async(&part).await;
        anyhow::bail!(
            "Cloud Node outbound cache copy failed integrity verification: {}",
            source.display()
        );
    }
    durable::rename_async(&part, target).await?;
    Ok(())
}

async fn cached_file_matches(
    path: &Path,
    expected_sha256: [u8; 32],
    expected_size: u64,
) -> anyhow::Result<bool> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() != expected_size {
        return Ok(false);
    }
    Ok(sha256_path(path).await? == expected_sha256)
}

pub(crate) fn cache_part_path(path: &Path) -> anyhow::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cloud Node cache target has no file name: {}",
                path.display()
            )
        })?
        .to_string_lossy();
    Ok(path.with_file_name(format!("{name}.part.{}", std::process::id())))
}

async fn write_cache_ready_marker(cache_root: &Path, header: SyncPlanHeader) -> anyhow::Result<()> {
    let target = cache_root.join(".ready");
    let part = cache_part_path(&target)?;
    if part.exists() {
        durable::remove_file_async(&part).await?;
    }
    tokio::fs::write(&part, header.encode()).await?;
    tokio::fs::OpenOptions::new().write(true).open(&part).await?.sync_all().await?;
    if target.exists() {
        durable::remove_file_async(&target).await?;
    }
    durable::rename_async(part, target).await?;
    Ok(())
}

async fn prune_outbound_cache_snapshots(
    peer_root: &Path,
    keep_snapshot: &str,
) -> anyhow::Result<()> {
    let mut entries = tokio::fs::read_dir(peer_root).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name().to_string_lossy() == keep_snapshot {
            continue;
        }
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            durable::remove_dir_all_async(entry.path()).await?;
        } else {
            durable::remove_file_async(entry.path()).await?;
        }
    }
    Ok(())
}

pub(crate) async fn cleanup_outbound_cache(
    store: &CloudNodeStore,
    peer_node_id: &str,
) -> anyhow::Result<()> {
    let root = outbound_peer_cache_root(store, peer_node_id);
    match durable::remove_dir_all_async(root).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_error) => {
            // Once a peer has proved activation, or a provider HEAD has been
            // published, this cache is only disposable local garbage. Leave it
            // in place for a later matching-root cleanup instead of changing the
            // already-committed synchronization result into a failure.
            Ok(())
        }
    }
}

pub(crate) async fn sha256_path(path: &Path) -> anyhow::Result<[u8; 32]> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}
