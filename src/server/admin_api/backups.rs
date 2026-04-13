use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tar::{Archive, Builder, Header};

#[derive(Debug, Deserialize)]
pub struct CreateBackupRequest {
    pub tenant: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BackupManifest {
    pub id: String,
    pub created_at: String,
    pub scope: String,
    pub tenant: Option<crate::store::models::Tenant>,
}

const BACKUP_INDEX_FILE: &str = "backup-index.json";

/// POST /api/backups — create a backup archive of tenant SQLite files
pub async fn create_backup(
    State(state): State<Arc<crate::server::AppState>>,
    Json(req): Json<CreateBackupRequest>,
) -> impl IntoResponse {
    let data_dir = std::path::Path::new(&state.data_dir);
    let backup_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let backup_dir = data_dir.join("backups");
    if let Err(e) = std::fs::create_dir_all(&backup_dir) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }

    let output_path = resolve_output_path(&backup_dir, req.output.as_deref(), &backup_id);
    let manifest = if let Some(ref tenant_ref) = req.tenant {
        let tenant = match super::resolve_tenant_id(&state.platform_store, tenant_ref) {
            Ok(id) => {
                let conn = state.platform_store.conn();
                match crate::store::models::Tenant::get_by_id(&conn, id) {
                    Ok(Some(t)) => t,
                    Ok(None) => return err(StatusCode::NOT_FOUND, "Tenant not found"),
                    Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
                }
            }
            Err((status, json)) => return (status, json),
        };
        BackupManifest {
            id: backup_id.clone(),
            created_at: now.clone(),
            scope: "tenant".to_string(),
            tenant: Some(tenant),
        }
    } else {
        BackupManifest {
            id: backup_id.clone(),
            created_at: now.clone(),
            scope: "instance".to_string(),
            tenant: None,
        }
    };

    let create_result = if manifest.scope == "tenant" {
        create_tenant_backup(data_dir, &output_path, &manifest)
    } else {
        create_instance_backup(data_dir, &output_path, &manifest)
    };

    let size_bytes = match create_result {
        Ok(size) => size,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    let info = BackupInfo {
        id: backup_id,
        filename: output_path.to_string_lossy().to_string(),
        size_bytes,
        created_at: now,
    };

    if let Err(e) = upsert_backup_index(&backup_dir, &info) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }

    (
        StatusCode::CREATED,
        Json(serde_json::to_value(&info).unwrap()),
    )
}

/// GET /api/backups — list available backups
pub async fn list_backups(State(state): State<Arc<crate::server::AppState>>) -> impl IntoResponse {
    let backup_dir = Path::new(&state.data_dir).join("backups");
    let backups = match read_backup_index(&backup_dir) {
        Ok(mut items) => {
            items.retain(|item| Path::new(&item.filename).exists());
            items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            items
        }
        Err(_) => Vec::new(),
    };
    Json(serde_json::to_value(&backups).unwrap())
}

/// POST /api/backups/restore — restore from a backup file
pub async fn restore_backup(
    State(state): State<Arc<crate::server::AppState>>,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let input = req
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let input_path = PathBuf::from(input);
    if !input_path.exists() {
        return err(
            StatusCode::NOT_FOUND,
            &format!("Backup file '{}' not found", input),
        )
        .into_response();
    }

    let tmp_dir = std::env::temp_dir().join(format!("hiveloom-restore-{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response();
    }

    let restore_result = (|| -> anyhow::Result<serde_json::Value> {
        let archive_file = File::open(&input_path)?;
        let decoder = GzDecoder::new(archive_file);
        let mut archive = Archive::new(decoder);
        archive.unpack(&tmp_dir)?;

        let manifest_bytes = std::fs::read(tmp_dir.join("backup.json"))?;
        let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes)?;
        let extracted_data = tmp_dir.join("data");
        let live_data_dir = Path::new(&state.data_dir);

        match manifest.scope.as_str() {
            "instance" => {
                copy_tree(&extracted_data, live_data_dir)?;
            }
            "tenant" => {
                let tenant_slug = manifest.tenant.as_ref().map(|t| t.slug.clone());
                let tenant = manifest
                    .tenant
                    .ok_or_else(|| anyhow::anyhow!("Tenant backup missing tenant metadata"))?;
                ensure_tenant_record(&state, &tenant)?;

                let extracted_tenant_dir =
                    extracted_data.join("tenants").join(tenant.id.to_string());
                if extracted_tenant_dir.exists() {
                    copy_tree(
                        &extracted_tenant_dir,
                        &live_data_dir.join("tenants").join(tenant.id.to_string()),
                    )?;
                }

                let extracted_master_key = extracted_data.join("master.key");
                if extracted_master_key.exists() {
                    copy_file(&extracted_master_key, &live_data_dir.join("master.key"))?;
                }

                return Ok(serde_json::json!({
                    "status": "restored",
                    "input": input,
                    "scope": manifest.scope,
                    "tenant": tenant_slug,
                }));
            }
            other => anyhow::bail!("Unsupported backup scope '{}'", other),
        }

        Ok(serde_json::json!({
            "status": "restored",
            "input": input,
            "scope": manifest.scope,
            "tenant": manifest.tenant.as_ref().map(|t| t.slug.clone()),
        }))
    })();

    let _ = std::fs::remove_dir_all(&tmp_dir);

    match restore_result {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

fn resolve_output_path(backup_dir: &Path, output: Option<&str>, backup_id: &str) -> PathBuf {
    match output {
        Some(path) if Path::new(path).is_absolute() => PathBuf::from(path),
        Some(path) => backup_dir.join(path),
        None => backup_dir.join(format!("hiveloom-backup-{}.tar.gz", backup_id)),
    }
}

fn create_instance_backup(
    data_dir: &Path,
    output_path: &Path,
    manifest: &BackupManifest,
) -> anyhow::Result<u64> {
    let mut items = Vec::new();
    for rel in [
        "master.key",
        "config.json",
        "platform.db",
        "platform.db-shm",
        "platform.db-wal",
    ] {
        let path = data_dir.join(rel);
        if path.exists() {
            items.push((path, PathBuf::from("data").join(rel)));
        }
    }

    for rel_dir in ["tenants", "manifests"] {
        let path = data_dir.join(rel_dir);
        if path.exists() {
            items.push((path, PathBuf::from("data").join(rel_dir)));
        }
    }

    write_backup_archive(output_path, manifest, &items)
}

fn create_tenant_backup(
    data_dir: &Path,
    output_path: &Path,
    manifest: &BackupManifest,
) -> anyhow::Result<u64> {
    let tenant = manifest
        .tenant
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Tenant backup requested without tenant metadata"))?;
    let mut items = Vec::new();

    let master_key = data_dir.join("master.key");
    if master_key.exists() {
        items.push((master_key, PathBuf::from("data").join("master.key")));
    }

    let tenant_dir = data_dir.join("tenants").join(tenant.id.to_string());
    if !tenant_dir.exists() {
        anyhow::bail!("Tenant store '{}' does not exist", tenant_dir.display());
    }
    items.push((
        tenant_dir,
        PathBuf::from("data")
            .join("tenants")
            .join(tenant.id.to_string()),
    ));

    write_backup_archive(output_path, manifest, &items)
}

fn write_backup_archive(
    output_path: &Path,
    manifest: &BackupManifest,
    items: &[(PathBuf, PathBuf)],
) -> anyhow::Result<u64> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(output_path)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);

    let manifest_bytes = serde_json::to_vec_pretty(manifest)?;
    append_bytes(&mut builder, Path::new("backup.json"), &manifest_bytes)?;

    for (src, dest) in items {
        append_path(&mut builder, src, dest)?;
    }

    builder.finish()?;
    let mut encoder = builder.into_inner()?;
    encoder.flush()?;
    let file = encoder.finish()?;
    file.sync_all()?;
    Ok(std::fs::metadata(output_path)?.len())
}

fn append_bytes<W: Write>(
    builder: &mut Builder<W>,
    dest: &Path,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let mut header = Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, dest, bytes)?;
    Ok(())
}

fn append_path<W: Write>(builder: &mut Builder<W>, src: &Path, dest: &Path) -> anyhow::Result<()> {
    if src.is_dir() {
        builder.append_dir(dest, src)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            append_path(builder, &entry.path(), &dest.join(entry.file_name()))?;
        }
    } else if src.is_file() {
        let mut file = File::open(src)?;
        builder.append_file(dest, &mut file)?;
    }
    Ok(())
}

fn read_backup_index(backup_dir: &Path) -> anyhow::Result<Vec<BackupInfo>> {
    let index_path = backup_dir.join(BACKUP_INDEX_FILE);
    if !index_path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(index_path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn upsert_backup_index(backup_dir: &Path, info: &BackupInfo) -> anyhow::Result<()> {
    let mut items = read_backup_index(backup_dir)?;
    items.retain(|item| item.id != info.id);
    items.push(BackupInfo {
        id: info.id.clone(),
        filename: info.filename.clone(),
        size_bytes: info.size_bytes,
        created_at: info.created_at.clone(),
    });
    let bytes = serde_json::to_vec_pretty(&items)?;
    std::fs::write(backup_dir.join(BACKUP_INDEX_FILE), bytes)?;
    Ok(())
}

fn ensure_tenant_record(
    state: &Arc<crate::server::AppState>,
    tenant: &crate::store::models::Tenant,
) -> anyhow::Result<()> {
    let conn = state.platform_store.conn();
    conn.execute(
        "INSERT INTO tenants (id, name, slug, timezone, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           slug = excluded.slug,
           timezone = excluded.timezone,
           status = excluded.status,
           updated_at = excluded.updated_at",
        rusqlite::params![
            tenant.id.to_string(),
            tenant.name,
            tenant.slug,
            tenant.timezone,
            tenant.status,
            tenant.created_at,
            tenant.updated_at,
        ],
    )?;
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> anyhow::Result<()> {
    if src.is_file() {
        copy_file(src, dest)?;
        return Ok(());
    }

    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_tree(&src_path, &dest_path)?;
        } else {
            copy_file(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

fn copy_file(src: &Path, dest: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dest)?;
    Ok(())
}
