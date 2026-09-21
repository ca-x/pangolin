use super::{
    Catalog, Model, Provider,
    types::{MAX_BYTES, invalid},
};
use crate::{api::ApiError, db};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Audit metadata injected into catalog mutations so the audit row is recorded inside the
/// same transaction — if the mutation rolls back, no audit trace remains.
pub struct CatalogAudit<'a> {
    pub actor_user_id: &'a str,
    pub action: &'a str,
}

fn sql(query: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, query, values)
}

#[derive(Clone, Debug, Serialize, FromQueryResult)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    pub priority: i32,
    pub refresh_interval_secs: i64,
    pub enabled: bool,
    pub signature_policy: String,
    pub public_key: Option<String>,
    pub revision: i64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    pub active_snapshot_id: Option<String>,
    pub previous_snapshot_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub name: String,
    pub url: String,
    pub priority: i32,
    pub refresh_interval_secs: i64,
    pub enabled: bool,
    pub signature_policy: String,
    pub public_key: Option<String>,
}
impl SourceInput {
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || !(60..=2592000).contains(&self.refresh_interval_secs)
        {
            return Err(invalid(
                "invalid source name or refresh interval (60–2592000 seconds)",
            ));
        }
        super::refresh::source_url(&self.url)?;
        if !matches!(
            self.signature_policy.as_str(),
            "none" | "optional" | "required"
        ) {
            return Err(invalid("invalid signature policy"));
        }
        if self.signature_policy == "required" && self.public_key.is_none() {
            return Err(invalid("required signatures need a pinned public key"));
        }
        if let Some(key) = &self.public_key {
            super::refresh::public_key(key)?;
        }
        Ok(())
    }
}

pub async fn sources(db: &DatabaseConnection) -> Result<Vec<Source>, ApiError> {
    Ok(Source::find_by_statement(sql(
        "SELECT * FROM catalog_sources ORDER BY priority DESC,id",
        vec![],
    ))
    .all(db)
    .await?)
}
pub async fn source(db: &DatabaseConnection, id: &str) -> Result<Source, ApiError> {
    Source::find_by_statement(sql(
        "SELECT * FROM catalog_sources WHERE id=?",
        vec![id.into()],
    ))
    .one(db)
    .await?
    .ok_or(ApiError::NotFound)
}

pub async fn create_source(
    db: &DatabaseConnection,
    input: SourceInput,
    audit: Option<CatalogAudit<'_>>,
) -> Result<Source, ApiError> {
    input.validate()?;
    let tx = db.begin().await?;
    let count = tx
        .query_one(sql("SELECT COUNT(*) AS count FROM catalog_sources", vec![]))
        .await?
        .unwrap()
        .try_get::<i64>("", "count")?;
    if count >= 32 {
        return Err(invalid("catalog source limit reached"));
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(sql("INSERT INTO catalog_sources(id,name,url,priority,refresh_interval_secs,enabled,signature_policy,public_key,revision) VALUES(?,?,?,?,?,?,?,?,1)",vec![id.clone().into(),input.name.into(),input.url.into(),input.priority.into(),input.refresh_interval_secs.into(),input.enabled.into(),input.signature_policy.into(),input.public_key.into()])).await?;
    if let Some(audit) = &audit {
        db::record_audit_event_in(
            &tx,
            audit.actor_user_id,
            audit.action,
            "catalog",
            &id,
            json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    source(db, &id).await
}

pub async fn update_source(
    db: &DatabaseConnection,
    id: &str,
    revision: i64,
    input: SourceInput,
    audit: Option<CatalogAudit<'_>>,
) -> Result<Source, ApiError> {
    input.validate()?;
    let tx = db.begin().await?;
    let result=tx.execute(sql("UPDATE catalog_sources SET name=?,url=?,priority=?,refresh_interval_secs=?,enabled=?,signature_policy=?,public_key=?,revision=revision+1,etag=NULL,last_modified=NULL WHERE id=? AND revision=?",vec![input.name.into(),input.url.into(),input.priority.into(),input.refresh_interval_secs.into(),input.enabled.into(),input.signature_policy.into(),input.public_key.into(),id.into(),revision.into()])).await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::Conflict(
            "catalog source changed; reload its revision".into(),
        ));
    }
    assemble(&tx).await?;
    if let Some(audit) = &audit {
        db::record_audit_event_in(
            &tx,
            audit.actor_user_id,
            audit.action,
            "catalog",
            id,
            json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    source(db, id).await
}

pub async fn delete_source(
    db: &DatabaseConnection,
    id: &str,
    audit: Option<CatalogAudit<'_>>,
) -> Result<(), ApiError> {
    let tx = db.begin().await?;
    if tx
        .execute(sql(
            "DELETE FROM catalog_sources WHERE id=?",
            vec![id.into()],
        ))
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::NotFound);
    }
    assemble(&tx).await?;
    if let Some(audit) = &audit {
        db::record_audit_event_in(
            &tx,
            audit.actor_user_id,
            audit.action,
            "catalog",
            id,
            json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn activate(
    db: &DatabaseConnection,
    current: &Source,
    body: &[u8],
    etag: Option<String>,
    last_modified: Option<String>,
    signature_verified: bool,
) -> Result<String, ApiError> {
    let document = Catalog::parse(body)?;
    let serialized = serde_json::to_string(&document).map_err(|e| ApiError::Internal(e.into()))?;
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let tx = db.begin().await?;
    tx.execute(sql("INSERT INTO catalog_snapshots(id,source_id,version,document_json,digest,source_url,signature_verified,signing_key,created_at) VALUES(?,?,?,?,?,?,?,?,?)",vec![id.clone().into(),current.id.clone().into(),document.version.into(),serialized.into(),blake3::hash(body).to_hex().to_string().into(),current.url.clone().into(),signature_verified.into(),current.public_key.clone().into(),timestamp.into()])).await?;
    let update=tx.execute(sql("UPDATE catalog_sources SET previous_snapshot_id=active_snapshot_id,active_snapshot_id=?,etag=?,last_modified=?,last_attempt_at=?,last_success_at=?,last_error=NULL,revision=revision+1 WHERE id=? AND revision=? AND enabled=1",vec![id.clone().into(),etag.into(),last_modified.into(),timestamp.into(),timestamp.into(),current.id.clone().into(),current.revision.into()])).await?;
    if update.rows_affected() != 1 {
        return Err(ApiError::Conflict(
            "catalog source changed during refresh".into(),
        ));
    }
    assemble(&tx).await?;
    // Keep a bounded history plus the current/previous snapshots. Activation and
    // history retention share the same transaction and revision fence.
    tx.execute(sql("DELETE FROM catalog_snapshots WHERE source_id=? AND id NOT IN (SELECT id FROM catalog_snapshots WHERE source_id=? ORDER BY created_at DESC,rowid DESC LIMIT 5) AND id NOT IN (SELECT active_snapshot_id FROM catalog_sources WHERE id=? UNION SELECT previous_snapshot_id FROM catalog_sources WHERE id=?)",vec![current.id.clone().into(),current.id.clone().into(),current.id.clone().into(),current.id.clone().into()])).await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn not_modified(db: &DatabaseConnection, current: &Source) -> Result<(), ApiError> {
    if current.active_snapshot_id.is_none()
        || (current.etag.is_none() && current.last_modified.is_none())
    {
        return Err(invalid(
            "source returned 304 without an active conditional snapshot",
        ));
    }
    let result=db.execute(sql("UPDATE catalog_sources SET last_attempt_at=?,last_success_at=?,last_error=NULL,revision=revision+1 WHERE id=? AND revision=? AND enabled=1",vec![db::now().into(),db::now().into(),current.id.clone().into(),current.revision.into()])).await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::Conflict(
            "catalog source changed during refresh".into(),
        ));
    }
    Ok(())
}

pub async fn failed(
    db: &DatabaseConnection,
    current: &Source,
    message: &str,
) -> Result<(), ApiError> {
    db.execute(sql(
        "UPDATE catalog_sources SET last_attempt_at=?,last_error=? WHERE id=? AND revision=?",
        vec![
            db::now().into(),
            message.chars().take(512).collect::<String>().into(),
            current.id.clone().into(),
            current.revision.into(),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn snapshots(db: &DatabaseConnection, id: &str) -> Result<Vec<Value>, ApiError> {
    source(db, id).await?;
    let rows=db.query_all(sql("SELECT id,version,digest,source_url,signature_verified,created_at FROM catalog_snapshots WHERE source_id=? ORDER BY created_at DESC,rowid DESC",vec![id.into()])).await?;
    rows.into_iter().map(|row|Ok(json!({"id":row.try_get::<String>("","id")?,"version":row.try_get::<String>("","version")?,"digest":row.try_get::<String>("","digest")?,"source_url":row.try_get::<String>("","source_url")?,"signature_verified":row.try_get::<bool>("","signature_verified")?,"created_at":row.try_get::<i64>("","created_at")?}))).collect()
}

pub async fn rollback(
    db: &DatabaseConnection,
    id: &str,
    snapshot_id: &str,
    revision: i64,
    audit: Option<CatalogAudit<'_>>,
) -> Result<(), ApiError> {
    let tx = db.begin().await?;
    let row = tx
        .query_one(sql(
            "SELECT document_json FROM catalog_snapshots WHERE id=? AND source_id=?",
            vec![snapshot_id.into(), id.into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    Catalog::parse(row.try_get::<String>("", "document_json")?.as_bytes())?;
    let result=tx.execute(sql("UPDATE catalog_sources SET previous_snapshot_id=active_snapshot_id,active_snapshot_id=?,etag=NULL,last_modified=NULL,revision=revision+1,last_error=NULL WHERE id=? AND revision=?",vec![snapshot_id.into(),id.into(),revision.into()])).await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::Conflict(
            "catalog source changed before rollback".into(),
        ));
    }
    assemble(&tx).await?;
    if let Some(audit) = &audit {
        db::record_audit_event_in(
            &tx,
            audit.actor_user_id,
            audit.action,
            "catalog",
            id,
            json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn import(
    db: &DatabaseConnection,
    body: &[u8],
    audit: Option<CatalogAudit<'_>>,
) -> Result<(usize, usize), ApiError> {
    let document = Catalog::parse(body)?;
    let counts = (document.providers.len(), document.models.len());
    let tx = db.begin().await?;
    for (key, value) in &document.extensions {
        tx.execute(sql("INSERT INTO catalog_document_extensions(extension_key,value_json,updated_at) VALUES(?,?,?) ON CONFLICT(extension_key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",vec![key.clone().into(),value.to_string().into(),db::now().into()])).await?;
    }
    for (kind, entries) in [
        (
            "provider",
            document
                .providers
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>(),
        ),
        (
            "model",
            document
                .models
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>(),
        ),
    ] {
        for entry in entries.map_err(|e| ApiError::Internal(e.into()))? {
            let id = entry["id"].as_str().unwrap();
            tx.execute(sql("INSERT INTO catalog_overrides(kind,entry_id,entry_json,updated_at) VALUES(?,?,?,?) ON CONFLICT(kind,entry_id) DO UPDATE SET entry_json=excluded.entry_json,updated_at=excluded.updated_at",vec![kind.into(),id.into(),entry.to_string().into(),db::now().into()])).await?;
        }
    }
    assemble(&tx).await?;
    if let Some(audit) = &audit {
        db::record_audit_event_in(
            &tx,
            audit.actor_user_id,
            audit.action,
            "catalog",
            "local-overrides",
            json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(counts)
}

pub async fn remove_override(
    db: &DatabaseConnection,
    kind: &str,
    id: &str,
    audit: Option<CatalogAudit<'_>>,
) -> Result<(), ApiError> {
    if !matches!(kind, "provider" | "model") {
        return Err(invalid("invalid override kind"));
    }
    let tx = db.begin().await?;
    if tx
        .execute(sql(
            "DELETE FROM catalog_overrides WHERE kind=? AND entry_id=?",
            vec![kind.into(), id.into()],
        ))
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::NotFound);
    }
    assemble(&tx).await?;
    if let Some(audit) = &audit {
        db::record_audit_event_in(
            &tx,
            audit.actor_user_id,
            audit.action,
            "catalog",
            id,
            json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn effective(db: &DatabaseConnection) -> Result<Catalog, ApiError> {
    let tx = db.begin().await?;
    let result = assemble(&tx).await?;
    tx.commit().await?;
    Ok(result)
}

async fn assemble<C: ConnectionTrait>(connection: &C) -> Result<Catalog, ApiError> {
    let rows=connection.query_all(sql("SELECT s.id,s.priority,p.document_json FROM catalog_sources s JOIN catalog_snapshots p ON p.id=s.active_snapshot_id AND p.source_id=s.id WHERE s.enabled=1 ORDER BY s.priority ASC,s.id ASC",vec![])).await?;
    let overrides = connection
        .query_all(sql(
            "SELECT kind,entry_json FROM catalog_overrides ORDER BY kind,entry_id",
            vec![],
        ))
        .await?;
    let mut result = super::builtin().clone();
    let mut providers: BTreeMap<String, Provider> = result
        .providers
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let mut models: BTreeMap<String, Model> = result
        .models
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let mut applied = vec![];
    for row in rows {
        let document = Catalog::parse(row.try_get::<String>("", "document_json")?.as_bytes())?;
        applied.push(json!({"id":row.try_get::<String>("","id")?,"version":document.version,"priority":row.try_get::<i32>("","priority")?}));
        providers.extend(document.providers.into_iter().map(|p| (p.id.clone(), p)));
        models.extend(document.models.into_iter().map(|m| (m.id.clone(), m)));
        result.extensions.extend(document.extensions);
    }
    for row in overrides {
        let entry: String = row.try_get("", "entry_json")?;
        match row.try_get::<String>("", "kind")?.as_str() {
            "provider" => {
                let p: Provider =
                    serde_json::from_str(&entry).map_err(|e| ApiError::Internal(e.into()))?;
                providers.insert(p.id.clone(), p);
            }
            "model" => {
                let m: Model =
                    serde_json::from_str(&entry).map_err(|e| ApiError::Internal(e.into()))?;
                models.insert(m.id.clone(), m);
            }
            _ => return Err(invalid("invalid stored catalog override")),
        }
    }
    result.providers = providers.into_values().collect();
    result.models = models.into_values().collect();
    for row in connection.query_all(sql("SELECT extension_key,value_json FROM catalog_document_extensions ORDER BY extension_key",vec![])).await? {
        let key:String=row.try_get("","extension_key")?;
        let value:String=row.try_get("","value_json")?;
        result.extensions.insert(key,serde_json::from_str(&value).map_err(|error|ApiError::Internal(error.into()))?);
    }
    // Runtime provenance is owned by the service, outside the extension map.
    // Feed/application extension keys must survive lossless JSON value roundtrips.
    result.source.revision = blake3::hash(
        json!({"builtin":super::builtin().version,"sources":applied})
            .to_string()
            .as_bytes(),
    )
    .to_hex()
    .to_string();
    result.validate()?;
    let bytes = serde_json::to_vec(&result).map_err(|e| ApiError::Internal(e.into()))?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("merged catalog exceeds export limit"));
    }
    result.version = format!("effective-{}", &blake3::hash(&bytes).to_hex()[..20]);
    Ok(result)
}

pub async fn due_sources(db: &DatabaseConnection, now: i64) -> Result<Vec<Source>, ApiError> {
    Ok(Source::find_by_statement(sql("SELECT * FROM catalog_sources WHERE enabled=1 AND (last_attempt_at IS NULL OR last_attempt_at+refresh_interval_secs<=?) ORDER BY priority DESC,id",vec![now.into()])).all(db).await?)
}
