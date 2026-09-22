//! Independent implementation of selective backup and Raindrop snapshot/fence contracts.
use super::{id, jobs, sql, storage};
use crate::{
    api::{ApiError, AppState},
    db,
};
use object_store::ObjectStoreExt;
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

// Dependency order; SQL identifiers are selected exclusively from this allowlist.
const TABLES: &[(&str, &str)] = &[
    ("providers", "project_id=?"),
    ("api_key_profiles", "project_id=?"),
    (
        "channel_credentials",
        "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
    ),
    (
        "channel_settings",
        "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
    ),
    (
        "models",
        "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
    ),
    ("model_associations", "project_id=?"),
    (
        "model_prices",
        "model_id IN (SELECT m.id FROM models m JOIN providers p ON p.id=m.provider_id WHERE p.project_id=?)",
    ),
    (
        "model_price_components",
        "price_id IN (SELECT mp.id FROM model_prices mp JOIN models m ON m.id=mp.model_id JOIN providers p ON p.id=m.provider_id WHERE p.project_id=?)",
    ),
    (
        "api_key_profile_model_mappings",
        "profile_id IN (SELECT id FROM api_key_profiles WHERE project_id=?)",
    ),
    (
        "api_key_profile_allowed_models",
        "profile_id IN (SELECT id FROM api_key_profiles WHERE project_id=?)",
    ),
    ("api_keys", "project_id=?"),
    ("prompts", "project_id=?"),
    ("prompt_protection_rules", "project_id=?"),
    ("service_groups", "project_id=?"),
    ("service_group_keys", "project_id=?"),
    (
        "service_group_channels",
        "group_id IN (SELECT id FROM service_groups WHERE project_id=?)",
    ),
    ("webhooks", "project_id=?"),
    ("data_retention_policies", "project_id=?"),
    ("threads", "project_id=?"),
    ("traces", "project_id=?"),
    (
        "requests",
        "trace_id IN (SELECT id FROM traces WHERE project_id=?)",
    ),
    (
        "request_contents",
        "request_id IN (SELECT r.id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE t.project_id=?)",
    ),
    ("request_facts", "project_id=?"),
    (
        "execution_facts",
        "request_id IN (SELECT id FROM request_facts WHERE project_id=?)",
    ),
    (
        "request_executions",
        "request_id IN (SELECT r.id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE t.project_id=?)",
    ),
    (
        "usage_logs",
        "execution_id IN (SELECT e.id FROM execution_facts e JOIN request_facts r ON r.id=e.request_id WHERE r.project_id=?)",
    ),
    (
        "usage_cost_items",
        "usage_log_id IN (SELECT u.id FROM usage_logs u JOIN execution_facts e ON e.id=u.execution_id JOIN request_facts r ON r.id=e.request_id WHERE r.project_id=?)",
    ),
    ("provider_response_settlements", "project_id=?"),
    ("response_sessions", "project_id=?"),
    ("session_summaries", "project_id=?"),
    (
        "provider_quota_snapshots",
        "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
    ),
    (
        "channel_probes",
        "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
    ),
];
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub resources: Vec<String>,
}
impl Selection {
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.resources.is_empty()
            || self.resources.len() > TABLES.len()
            || self
                .resources
                .iter()
                .any(|name| !TABLES.iter().any(|(t, _)| t == name))
        {
            return Err(ApiError::BadRequest(
                "unknown or empty backup resource selection".into(),
            ));
        }
        Ok(())
    }
}
/// The restorable document for one stored artifact.
///
/// The console's restore parser requires `version`, `project_id` and `resources`
/// alongside the envelope, so every route that hands an artifact to a client must
/// build it here rather than return a subset of the row.
pub async fn read_artifact(
    db: &sea_orm::DatabaseConnection,
    id: &str,
    project: &str,
) -> Result<Option<Artifact>, ApiError> {
    let row = db
        .query_one(sql(
            "SELECT id,project_id,digest,manifest_json,envelope,created_at FROM backup_artifacts WHERE id=? AND project_id=?",
            vec![id.into(), project.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    let manifest: Value = serde_json::from_str(&row.try_get::<String>("", "manifest_json")?)
        .map_err(|e| ApiError::Internal(e.into()))?;
    Ok(Some(Artifact {
        version: 1,
        id: row.try_get("", "id")?,
        project_id: row.try_get("", "project_id")?,
        created_at: row.try_get("", "created_at")?,
        digest: row.try_get("", "digest")?,
        envelope: row.try_get("", "envelope")?,
        resources: serde_json::from_value(manifest["resources"].clone())
            .map_err(|e| ApiError::Internal(e.into()))?,
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub version: u32,
    pub id: String,
    pub project_id: String,
    pub created_at: i64,
    pub digest: String,
    pub resources: Vec<String>,
    pub envelope: String,
}
#[derive(Deserialize, Serialize)]
struct Contents {
    version: u32,
    artifact_id: String,
    project_id: String,
    tables: BTreeMap<String, Vec<Value>>,
}
async fn columns(db: &impl ConnectionTrait, table: &str) -> Result<Vec<(String, bool)>, ApiError> {
    Ok(db
        .query_all(sql(format!("PRAGMA table_info({table})"), vec![]))
        .await?
        .into_iter()
        .map(|r| Ok((r.try_get("", "name")?, r.try_get::<i64>("", "pk")? > 0)))
        .collect::<Result<_, sea_orm::DbErr>>()?)
}
async fn rows(
    db: &impl ConnectionTrait,
    table: &str,
    scope: &str,
    project: &str,
) -> Result<Vec<Value>, ApiError> {
    let columns = columns(db, table).await?;
    let expression = columns
        .iter()
        .map(|(name, _)| format!("'{name}',\"{name}\""))
        .collect::<Vec<_>>()
        .join(",");
    let size=db.query_one(sql(format!("SELECT COALESCE(SUM(length(json_object({expression}))),0) AS bytes,COUNT(*) AS count FROM {table} WHERE {scope}"),vec![project.into()])).await?.ok_or(ApiError::NotFound)?;
    if size.try_get::<i64>("", "bytes")? > 32 * 1024 * 1024
        || size.try_get::<i64>("", "count")? > 100000
    {
        return Err(ApiError::BadRequest(
            "backup resource exceeds snapshot limits".into(),
        ));
    }
    let rows=db.query_all(sql(format!("SELECT json_object({expression}) AS document FROM {table} WHERE {scope} LIMIT 100001"),vec![project.into()])).await?;
    if rows.len() > 100000 {
        return Err(ApiError::BadRequest(
            "backup resource exceeds row limit".into(),
        ));
    }
    rows.into_iter()
        .map(|r| {
            serde_json::from_str(&r.try_get::<String>("", "document")?)
                .map_err(|e| ApiError::Internal(e.into()))
        })
        .collect()
}
pub async fn export(
    state: &AppState,
    project: &str,
    selection: &Selection,
) -> Result<Artifact, ApiError> {
    export_as(state, project, selection, None).await
}
pub async fn export_as(
    state: &AppState,
    project: &str,
    selection: &Selection,
    actor: Option<&crate::access::Principal>,
) -> Result<Artifact, ApiError> {
    selection.validate()?;
    let artifact_id = id();
    let tx = state.db.begin().await?;
    let mut tables = BTreeMap::new();
    let mut total_bytes = 0usize;
    for (table, scope) in TABLES {
        if selection.resources.iter().any(|s| s == table) {
            let records = rows(&tx, table, scope, project).await?;
            total_bytes += serde_json::to_vec(&records)
                .map_err(|e| ApiError::Internal(e.into()))?
                .len();
            if total_bytes > 32 * 1024 * 1024 {
                return Err(ApiError::BadRequest("backup exceeds 32 MiB".into()));
            }
            tables.insert(table.to_string(), records);
        }
    }
    let contents = Contents {
        version: 1,
        artifact_id: artifact_id.clone(),
        project_id: project.into(),
        tables,
    };
    let encoded = serde_json::to_vec(&contents).map_err(|e| ApiError::Internal(e.into()))?;
    if encoded.len() > 32 * 1024 * 1024 {
        return Err(ApiError::BadRequest("backup exceeds 32 MiB".into()));
    }
    let digest = blake3::hash(&encoded).to_hex().to_string();
    let envelope = state
        .secrets
        .encrypt(std::str::from_utf8(&encoded).unwrap())?;
    let artifact = Artifact {
        version: 1,
        id: artifact_id,
        project_id: project.into(),
        created_at: db::now(),
        digest,
        resources: selection.resources.clone(),
        envelope,
    };
    tx.execute(sql("INSERT INTO backup_artifacts(id,project_id,digest,manifest_json,envelope,created_at) VALUES(?,?,?,?,?,?)",vec![artifact.id.clone().into(),project.into(),artifact.digest.clone().into(),json!({"version":1,"resources":artifact.resources}).to_string().into(),artifact.envelope.clone().into(),artifact.created_at.into()])).await?;
    super::audit(&tx, actor, project, "backup.export", &artifact.id).await?;
    tx.commit().await?;
    Ok(artifact)
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    Fail,
    Skip,
    Overwrite,
}
impl Conflict {
    fn name(self) -> &'static str {
        match self {
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
        }
    }
}
#[cfg(test)]
pub async fn restore(
    state: &AppState,
    project: &str,
    artifact: &Artifact,
    strategy: Conflict,
) -> Result<usize, ApiError> {
    restore_as(state, project, artifact, strategy, None).await
}
pub async fn restore_as(
    state: &AppState,
    project: &str,
    artifact: &Artifact,
    strategy: Conflict,
    actor: Option<&crate::access::Principal>,
) -> Result<usize, ApiError> {
    if artifact.version != 1
        || artifact.project_id != project
        || artifact.envelope.len() > 48 * 1024 * 1024
    {
        return Err(ApiError::BadRequest(
            "backup version, project or size is invalid".into(),
        ));
    }
    let plaintext = state.secrets.decrypt(&artifact.envelope)?;
    if blake3::hash(plaintext.as_bytes()).to_hex().as_str() != artifact.digest {
        return Err(ApiError::BadRequest("backup checksum mismatch".into()));
    }
    let contents: Contents = serde_json::from_str(&plaintext)
        .map_err(|_| ApiError::BadRequest("invalid backup document".into()))?;
    if contents.version != 1
        || contents.project_id != project
        || contents.artifact_id != artifact.id
        || contents.tables.keys().cloned().collect::<BTreeSet<_>>()
            != artifact.resources.iter().cloned().collect()
    {
        return Err(ApiError::BadRequest("backup binding mismatch".into()));
    }
    Selection {
        resources: artifact.resources.clone(),
    }
    .validate()?;
    let tx = state.db.begin().await?;
    let inserted=tx.execute(sql("INSERT INTO backup_restores(artifact_id,project_id,strategy,restored_at) VALUES(?,?,?,?) ON CONFLICT DO NOTHING",vec![artifact.id.clone().into(),project.into(),strategy.name().into(),db::now().into()])).await?.rows_affected();
    if inserted == 0 {
        tx.rollback().await?;
        return Ok(0);
    }
    let mut count = 0;
    let mut imported_requests = vec![];
    let mut imported_executions = vec![];
    let mut remappings = BTreeMap::<String, String>::new();
    for (table, scope) in TABLES {
        let Some(records) = contents.tables.get(*table) else {
            continue;
        };
        let columns = columns(&tx, table).await?;
        let valid = columns
            .iter()
            .map(|v| v.0.as_str())
            .collect::<BTreeSet<_>>();
        let primary = columns
            .iter()
            .filter(|v| v.1)
            .map(|v| v.0.as_str())
            .collect::<Vec<_>>();
        for record in records {
            let mut staged = record.clone();
            let object = staged
                .as_object_mut()
                .ok_or_else(|| ApiError::BadRequest("backup row must be an object".into()))?;
            for (name, item) in object.iter_mut() {
                if matches!(
                    name.as_str(),
                    "provider_id"
                        | "model_id"
                        | "profile_id"
                        | "group_id"
                        | "api_key_id"
                        | "price_id"
                ) && let Some(mapped) = item.as_str().and_then(|s| remappings.get(s))
                {
                    *item = json!(mapped);
                }
                if name.ends_with("_json")
                    && let Some(document) = item.as_str()
                    && let Ok(mut value) = serde_json::from_str::<Value>(document)
                {
                    let original = value.clone();
                    remap_document(&mut value, &remappings);
                    if value != original {
                        *item = Value::String(value.to_string());
                    }
                }
            }
            let natural: &[&str] = match *table {
                "providers" => &["name"],
                "models" => &["provider_id", "public_name", "upstream_name"],
                "api_key_profiles" | "prompts" | "service_groups" => &["project_id", "name"],
                "api_key_profile_model_mappings" => &["profile_id", "source_model"],
                _ => &[],
            };
            if !natural.is_empty() {
                let clause = natural
                    .iter()
                    .map(|name| format!("\"{name}\"=?"))
                    .collect::<Vec<_>>()
                    .join(" AND ");
                let values = natural
                    .iter()
                    .map(|name| value(object.get(*name).unwrap_or(&Value::Null)))
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(existing) = tx
                    .query_one(sql(
                        format!("SELECT id FROM {table} WHERE {clause}"),
                        values,
                    ))
                    .await?
                {
                    let existing: String = existing.try_get("", "id")?;
                    let original = object
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or(ApiError::NotFound)?
                        .to_owned();
                    if original != existing {
                        remappings.insert(original, existing.clone());
                        object.insert("id".into(), json!(existing));
                    }
                }
            }
            if object.len() != valid.len()
                || object.keys().any(|k| !valid.contains(k.as_str()))
                || object.get("project_id").is_some_and(|p| p != project)
            {
                return Err(ApiError::BadRequest(
                    "backup row schema or scope mismatch".into(),
                ));
            }
            // IDs may conflict with existing resources but may never overwrite another project.
            let where_pk = primary
                .iter()
                .map(|key| format!("\"{key}\"=?"))
                .collect::<Vec<_>>()
                .join(" AND ");
            let pk_values = primary
                .iter()
                .map(|key| value(&object[*key]))
                .collect::<Result<Vec<_>, _>>()?;
            let existing = tx
                .query_one(sql(
                    format!("SELECT 1 AS found FROM {table} WHERE {where_pk}"),
                    pk_values.clone(),
                ))
                .await?
                .is_some();
            if existing {
                let mut args = pk_values;
                args.push(project.into());
                if tx
                    .query_one(sql(
                        format!("SELECT 1 AS found FROM {table} WHERE {where_pk} AND ({scope})"),
                        args,
                    ))
                    .await?
                    .is_none()
                {
                    return Err(ApiError::Forbidden);
                }
                if strategy == Conflict::Skip {
                    continue;
                }
                if strategy == Conflict::Fail {
                    return Err(ApiError::Conflict(format!(
                        "backup resource {table} already exists"
                    )));
                }
                if matches!(
                    *table,
                    "model_prices"
                        | "model_price_components"
                        | "request_facts"
                        | "execution_facts"
                        | "usage_logs"
                        | "usage_cost_items"
                        | "provider_response_settlements"
                        | "session_summaries"
                ) {
                    // Immutable accounting versions are never reinterpreted by restore.
                    let expression = columns
                        .iter()
                        .map(|(name, _)| format!("'{name}',\"{name}\""))
                        .collect::<Vec<_>>()
                        .join(",");
                    let values = primary
                        .iter()
                        .map(|key| value(&object[*key]))
                        .collect::<Result<Vec<_>, _>>()?;
                    let current=tx.query_one(sql(format!("SELECT json_object({expression}) AS document FROM {table} WHERE {where_pk}"),values)).await?.ok_or(ApiError::NotFound)?;
                    let current: Value =
                        serde_json::from_str(&current.try_get::<String>("", "document")?)
                            .map_err(|e| ApiError::Internal(e.into()))?;
                    if current.as_object() == Some(object) {
                        continue;
                    } else {
                        return Err(ApiError::Conflict(
                            "cannot overwrite immutable price history".into(),
                        ));
                    }
                }
            }
            let names = object
                .keys()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(",");
            let placeholders = vec!["?"; object.len()].join(",");
            let update = object
                .keys()
                .filter(|s| !primary.contains(&s.as_str()))
                .map(|s| {
                    if *table == "api_keys" && s == "spent_micros" {
                        "spent_micros=MAX(api_keys.spent_micros,excluded.spent_micros)".into()
                    } else {
                        format!("\"{s}\"=excluded.\"{s}\"")
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            let conflict = if strategy == Conflict::Overwrite && !update.is_empty() {
                format!(" ON CONFLICT({}) DO UPDATE SET {update}", primary.join(","))
            } else {
                String::new()
            };
            tx.execute(sql(
                format!("INSERT INTO {table}({names}) VALUES({placeholders}){conflict}"),
                object.values().map(value).collect::<Result<Vec<_>, _>>()?,
            ))
            .await?;
            count += 1;
            if !existing && matches!(*table, "request_facts" | "execution_facts") {
                let id = object
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or(ApiError::NotFound)?
                    .to_owned();
                if *table == "request_facts" {
                    imported_requests.push(id);
                } else {
                    imported_executions.push(id);
                }
            }
            // Validate every restored relationship with the same scoped selection used by export.
            let mut args = primary
                .iter()
                .map(|key| value(&object[*key]))
                .collect::<Result<Vec<_>, _>>()?;
            args.push(project.into());
            if tx
                .query_one(sql(
                    format!("SELECT 1 AS found FROM {table} WHERE {where_pk} AND ({scope})"),
                    args,
                ))
                .await?
                .is_none()
            {
                return Err(ApiError::Forbidden);
            }
        }
    }
    super::runtime::recover_in(&tx, Some(&imported_executions), Some(&imported_requests)).await?;
    super::audit(&tx, actor, project, "backup.restore", &artifact.id).await?;
    tx.commit().await?;
    Ok(count)
}
fn remap_document(value: &mut Value, remappings: &BTreeMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(mapped) = remappings.get(text) {
                *text = mapped.clone()
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                remap_document(value, remappings)
            }
        }
        Value::Array(array) => {
            for value in array {
                remap_document(value, remappings)
            }
        }
        _ => (),
    }
}
fn value(value: &Value) -> Result<sea_orm::Value, ApiError> {
    match value {
        Value::Null => Ok(Option::<String>::None.into()),
        Value::String(s) => Ok(s.clone().into()),
        Value::Number(n) => n
            .as_i64()
            .map(Into::into)
            .ok_or_else(|| ApiError::BadRequest("backup numbers must be integers".into())),
        _ => Err(ApiError::BadRequest("invalid backup scalar".into())),
    }
}

pub async fn enqueue(
    state: &AppState,
    project: &str,
    targets: &[String],
    selection: &Selection,
    operation: &str,
) -> Result<String, ApiError> {
    let tx = state.db.begin().await?;
    let job = enqueue_in(&tx, project, targets, selection, operation).await?;
    super::audit(&tx, None, project, "backup.enqueue", &job).await?;
    tx.commit().await?;
    Ok(job)
}
pub async fn enqueue_in(
    tx: &impl ConnectionTrait,
    project: &str,
    targets: &[String],
    selection: &Selection,
    operation: &str,
) -> Result<String, ApiError> {
    selection.validate()?;
    if targets.is_empty()
        || targets.len() > 16
        || targets.iter().collect::<BTreeSet<_>>().len() != targets.len()
    {
        return Err(ApiError::BadRequest(
            "choose 1–16 unique backup targets".into(),
        ));
    }
    let job = jobs::enqueue(
        tx,
        Some(project),
        "backup",
        operation,
        &json!({"resources":selection.resources}),
        db::now(),
    )
    .await?;
    for target in targets {
        let row=tx.query_one(sql("SELECT id,kind,config_json,secret_envelope,revision FROM data_storage_configs WHERE id=? AND project_id=? AND enabled=1",vec![target.clone().into(),project.into()])).await?.ok_or(ApiError::NotFound)?;
        let config: storage::Config =
            serde_json::from_str(&row.try_get::<String>("", "config_json")?)
                .map_err(|e| ApiError::Internal(e.into()))?;
        storage::validate(&config)?;
        let revision: i64 = row.try_get("", "revision")?;
        let snapshot = serde_json::to_string(&config).map_err(|e| ApiError::Internal(e.into()))?;
        let object_key = format!(
            "{}{job}.json",
            storage::owned_prefix(&config, project, target)?
        );
        tx.execute(sql("INSERT INTO backup_job_targets(id,job_id,storage_id,revision,snapshot_json,secret_envelope,object_key) VALUES(?,?,?,?,?,?,?) ON CONFLICT(job_id,storage_id) DO NOTHING",vec![id().into(),job.clone().into(),target.clone().into(),revision.into(),snapshot.into(),row.try_get::<Option<String>>("","secret_envelope")?.into(),object_key.into()])).await?;
    }
    Ok(job)
}
pub async fn execute(state: &AppState, claim: &jobs::Claim) -> Result<(), ApiError> {
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    let payload: Value =
        serde_json::from_str(&claim.payload_json).map_err(|e| ApiError::Internal(e.into()))?;
    let retention = payload["retention_count"]
        .as_i64()
        .filter(|keep| (1..=1000).contains(keep));
    // Automatic retries reuse the original immutable artifact, never a later DB snapshot.
    let artifact = if let Some(row) = state
        .db
        .query_one(sql(
            "SELECT payload_json FROM operation_jobs WHERE id=?",
            vec![claim.id.clone().into()],
        ))
        .await?
    {
        let v: Value = serde_json::from_str(&row.try_get::<String>("", "payload_json")?)
            .map_err(|e| ApiError::Internal(e.into()))?;
        if let Some(artifact) = v["artifact_id"].as_str() {
            let row=state.db.query_one(sql("SELECT id,project_id,digest,manifest_json,envelope,created_at FROM backup_artifacts WHERE id=? AND project_id=?",vec![artifact.into(),project.into()])).await?.ok_or(ApiError::NotFound)?;
            let manifest: Value =
                serde_json::from_str(&row.try_get::<String>("", "manifest_json")?)
                    .map_err(|e| ApiError::Internal(e.into()))?;
            Some(Artifact {
                version: 1,
                id: row.try_get("", "id")?,
                project_id: row.try_get("", "project_id")?,
                created_at: row.try_get("", "created_at")?,
                digest: row.try_get("", "digest")?,
                envelope: row.try_get("", "envelope")?,
                resources: serde_json::from_value(manifest["resources"].clone())
                    .map_err(|e| ApiError::Internal(e.into()))?,
            })
        } else {
            None
        }
    } else {
        None
    };
    let artifact = if let Some(artifact) = artifact {
        artifact
    } else {
        let selection = Selection {
            resources: serde_json::from_value(payload["resources"].clone())
                .map_err(|_| ApiError::BadRequest("invalid backup resources".into()))?,
        };
        let artifact = export(state, project, &selection).await?;
        let mut updated = payload;
        updated["artifact_id"] = json!(artifact.id);
        let changed=state.db.execute(sql("UPDATE operation_jobs SET payload_json=? WHERE id=? AND owner=? AND fence=? AND status='running' AND lease_until>?",vec![updated.to_string().into(),claim.id.clone().into(),claim.owner.clone().into(),claim.fence.into(),db::now().into()])).await?.rows_affected();
        if changed != 1 {
            return Err(ApiError::Conflict("backup lease expired".into()));
        }
        artifact
    };
    let bytes = serde_json::to_vec(&artifact).map_err(|e| ApiError::Internal(e.into()))?;
    let mut failed = false;
    let targets=state.db.query_all(sql("SELECT id,storage_id,revision,snapshot_json,secret_envelope,object_key FROM backup_job_targets WHERE job_id=? AND status!='succeeded'",vec![claim.id.clone().into()])).await?;
    for row in targets {
        let target: String = row.try_get("", "storage_id")?;
        let revision: i64 = row.try_get("", "revision")?;
        let result_id: String = row.try_get("", "id")?;
        let current=state.db.query_one(sql("SELECT 1 AS valid FROM data_storage_configs WHERE id=? AND project_id=? AND enabled=1 AND revision=?",vec![target.clone().into(),project.into(),revision.into()])).await?.is_some();
        let result = if current {
            let config: storage::Config =
                serde_json::from_str(&row.try_get::<String>("", "snapshot_json")?)
                    .map_err(|e| ApiError::Internal(e.into()))?;
            let secret: Option<String> = row.try_get("", "secret_envelope")?;
            let key: String = row.try_get("", "object_key")?;
            let store = storage::open(state, project, &target, &config, secret.as_deref()).await;
            match store {
                Ok(store) => storage::put(store.as_ref(), &key, bytes.clone()).await,
                Err(e) => Err(e),
            }
        } else {
            Err(ApiError::Conflict("target revision changed".into()))
        };
        let tx = state.db.begin().await?;
        jobs::fence(&tx, claim).await?;
        let current=current&&tx.query_one(sql("SELECT 1 AS valid FROM data_storage_configs WHERE id=? AND project_id=? AND enabled=1 AND revision=?",vec![target.clone().into(),project.into(),revision.into()])).await?.is_some();
        let success = result.is_ok() && current;
        failed |= !success;
        let changed=tx.execute(sql("UPDATE backup_job_targets SET status=?,error_code=?,byte_size=? WHERE id=? AND EXISTS(SELECT 1 FROM operation_jobs WHERE id=? AND owner=? AND fence=? AND status='running' AND lease_until>?)",vec![if success{"succeeded"}else{"failed"}.into(),if success{None}else if !current{Some("target_changed")}else{Some("target_unreachable")}.into(),success.then_some(bytes.len() as i64).into(),result_id.into(),claim.id.clone().into(),claim.owner.clone().into(),claim.fence.into(),db::now().into()])).await?.rows_affected();
        if changed != 1 {
            return Err(ApiError::Conflict("backup lease expired".into()));
        }
        if success && let Some(keep) = retention {
            jobs::enqueue(
                &tx,
                Some(project),
                "backup_retention",
                &format!("retention:{}:{target}", claim.id),
                &json!({"storage_id":target,"keep":keep}),
                db::now(),
            )
            .await?;
        }
        tx.commit().await?;
    }
    if failed {
        Err(ApiError::Upstream(
            "one or more backup targets failed".into(),
        ))
    } else {
        Ok(())
    }
}
#[cfg(test)]
pub async fn retry(state: &AppState, project: &str, job: &str) -> Result<String, ApiError> {
    retry_as(state, project, job, None).await
}
pub async fn retry_as(
    state: &AppState,
    project: &str,
    job: &str,
    actor: Option<&crate::access::Principal>,
) -> Result<String, ApiError> {
    let row = state
        .db
        .query_one(sql(
            "SELECT payload_json FROM operation_jobs WHERE id=? AND project_id=? AND kind='backup'",
            vec![job.into(), project.into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let payload: Value = serde_json::from_str(&row.try_get::<String>("", "payload_json")?)
        .map_err(|e| ApiError::Internal(e.into()))?;
    let targets = state
        .db
        .query_all(sql(
            "SELECT storage_id FROM backup_job_targets WHERE job_id=?",
            vec![job.into()],
        ))
        .await?
        .into_iter()
        .map(|r| r.try_get::<String>("", "storage_id"))
        .collect::<Result<Vec<_>, _>>()?;
    let tx = state.db.begin().await?;
    let job = enqueue_in(
        &tx,
        project,
        &targets,
        &Selection {
            resources: serde_json::from_value(payload["resources"].clone())
                .map_err(|e| ApiError::Internal(e.into()))?,
        },
        &format!("manual:{}", id()),
    )
    .await?;
    super::audit(&tx, actor, project, "backup.retry", &job).await?;
    tx.commit().await?;
    Ok(job)
}
pub async fn retain(
    state: &AppState,
    project: &str,
    target: &str,
    keep: usize,
) -> Result<usize, ApiError> {
    if !(1..=1000).contains(&keep) {
        return Err(ApiError::BadRequest(
            "retention count must be 1–1000".into(),
        ));
    }
    let row=state.db.query_one(sql("SELECT config_json,secret_envelope,revision FROM data_storage_configs WHERE id=? AND project_id=? AND enabled=1",vec![target.into(),project.into()])).await?.ok_or(ApiError::NotFound)?;
    let config: storage::Config = serde_json::from_str(&row.try_get::<String>("", "config_json")?)
        .map_err(|e| ApiError::Internal(e.into()))?;
    let prefix = storage::owned_prefix(&config, project, target)?;
    let store = storage::open(
        state,
        project,
        target,
        &config,
        row.try_get::<Option<String>>("", "secret_envelope")?
            .as_deref(),
    )
    .await?;
    // Delete only successful objects recorded by this instance and target revision,
    // never arbitrary objects returned by a bucket listing.
    let rows=state.db.query_all(sql("SELECT t.id,t.object_key FROM backup_job_targets t JOIN operation_jobs j ON j.id=t.job_id WHERE t.storage_id=? AND t.revision=? AND j.project_id=? AND t.status='succeeded' ORDER BY j.created_at DESC,t.id DESC LIMIT 100 OFFSET ?",vec![target.into(),row.try_get::<i64>("","revision")?.into(),project.into(),(keep as i64).into()])).await?;
    let mut removed = 0;
    for row in rows {
        let key: String = row.try_get("", "object_key")?;
        let Some(name) = key.strip_prefix(&prefix) else {
            return Err(storage::invalid());
        };
        let Some(uuid) = name.strip_suffix(".json") else {
            return Err(storage::invalid());
        };
        if uuid::Uuid::parse_str(uuid).is_err() {
            return Err(storage::invalid());
        }
        match store
            .delete(&object_store::path::Path::parse(&key).map_err(|_| storage::invalid())?)
            .await
        {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => (),
            Err(e) => return Err(ApiError::Internal(e.into())),
        }
        state
            .db
            .execute(sql(
                "UPDATE backup_job_targets SET status='retained_out' WHERE id=?",
                vec![row.try_get::<String>("", "id")?.into()],
            ))
            .await?;
        removed += 1;
    }
    Ok(removed)
}
