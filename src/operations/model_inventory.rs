//! Credential-scoped discovery facts. Only revision-matching snapshots participate.
use super::{jobs, sql};
use crate::{
    api::{ApiError, AppState},
    catalog::types::{ModelCardProjection, StoredModelMetadata},
    db,
    models::RouteTarget,
};
use futures_util::{StreamExt, stream};
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub struct Credential {
    pub id: String,
    pub provider: String,
    pub fingerprint: String,
    pub config_fingerprint: String,
    pub target: RouteTarget,
}

/// Hash credential identity and the inputs actually used by discovery. Labels,
/// priorities, routing policy and generic wall-clock update times are not revisions.
/// Envelopes remain opaque; no plaintext secret enters a fingerprint.
pub async fn credentials(
    db: &impl ConnectionTrait,
    project: &str,
) -> Result<Vec<Credential>, ApiError> {
    let rows = db.query_all(sql(r#"SELECT c.id,c.provider_id,c.credential_type,c.secret_envelope,
        p.name,p.kind,p.base_url,s.proxy_url,s.proxy_username,s.proxy_secret_envelope,COALESCE(s.proxy_reuse_connections,1) AS proxy_reuse_connections,s.proxy_preset_id,
        json_object('kind',p.kind,'base_url',p.base_url,
          'oauth',CASE WHEN ? THEN json_extract(p.settings_json,'$.oauth_test') ELSE NULL END,
          'proxy',CASE WHEN s.proxy_url IS NOT NULL THEN
            json_object('url',s.proxy_url,'username',s.proxy_username,'secret',s.proxy_secret_envelope,'reuse',COALESCE(s.proxy_reuse_connections,1))
          WHEN s.proxy_preset_id IS NOT NULL THEN
            json_object('id',s.proxy_preset_id,'url',proxy.url,'secret',proxy.secret_envelope,'enabled',proxy.enabled)
          ELSE NULL END) AS config
        FROM channel_credentials c JOIN providers p ON p.id=c.provider_id AND p.enabled=1
        JOIN projects project ON project.id=p.project_id AND project.enabled=1
        LEFT JOIN channel_settings s ON s.provider_id=p.id LEFT JOIN proxy_presets proxy ON proxy.id=s.proxy_preset_id
        WHERE p.project_id=? AND c.enabled=1 ORDER BY c.priority,c.id"#, vec![cfg!(test).into(),project.into()])).await?;
    rows.into_iter()
        .map(|row| {
            let secret: String = row.try_get("", "secret_envelope")?;
            let kind: String = row.try_get("", "credential_type")?;
            let config: String = row.try_get("", "config")?;
            Ok(Credential {
                id: row.try_get("", "id")?,
                provider: row.try_get("", "provider_id")?,
                fingerprint: blake3::hash(json!([secret, kind]).to_string().as_bytes())
                    .to_hex()
                    .to_string(),
                config_fingerprint: blake3::hash(config.as_bytes()).to_hex().to_string(),
                target: RouteTarget {
                    public_name: String::new(),
                    upstream_name: String::new(),
                    provider_name: row.try_get("", "name")?,
                    provider_kind: row.try_get("", "kind")?,
                    base_url: row.try_get("", "base_url")?,
                    credential_type: kind,
                    secret_envelope: secret,
                    proxy_url: row.try_get("", "proxy_url")?,
                    proxy_username: row.try_get("", "proxy_username")?,
                    proxy_secret_envelope: row.try_get("", "proxy_secret_envelope")?,
                    proxy_reuse_connections: row.try_get("", "proxy_reuse_connections")?,
                    proxy_preset_id: row.try_get("", "proxy_preset_id")?,
                },
            })
        })
        .collect()
}

pub struct Snapshot {
    pub provider: String,
    pub status: String,
    pub last_success_at: Option<i64>,
    pub error_code: Option<String>,
    pub models: BTreeMap<String, Option<ModelCardProjection>>,
}
#[derive(Default)]
pub struct Inventory(pub BTreeMap<String, Snapshot>);
impl Inventory {
    pub fn allows(&self, credential: &str, provider: &str, model: &str, managed: bool) -> bool {
        let current = self
            .0
            .get(credential)
            .filter(|snapshot| snapshot.last_success_at.is_some());
        if managed {
            return current.is_some_and(|snapshot| snapshot.models.contains_key(model));
        }
        let confirmed = self.0.values().any(|snapshot| {
            snapshot.provider == provider
                && snapshot.last_success_at.is_some()
                && snapshot.models.contains_key(model)
        });
        !confirmed || current.is_none_or(|snapshot| snapshot.models.contains_key(model))
    }
    pub fn card(&self, credential: &str, model: &str) -> Option<&Option<ModelCardProjection>> {
        self.0
            .get(credential)
            .filter(|snapshot| snapshot.last_success_at.is_some())?
            .models
            .get(model)
    }
    pub fn credential_metadata(&self, credential: &str) -> Value {
        let snapshot = self.0.get(credential);
        json!({"discovery_status":snapshot.map(|s|s.status.as_str()).unwrap_or("unknown"),
            "discovery_model_count":snapshot.filter(|s|s.last_success_at.is_some()).map(|s|s.models.len()),
            "discovery_last_success_at":snapshot.and_then(|s|s.last_success_at),
            "discovery_error_code":snapshot.and_then(|s|s.error_code.as_deref())})
    }
}

pub async fn load(db: &impl ConnectionTrait, project: &str) -> Result<Inventory, ApiError> {
    let current = credentials(db, project).await?;
    load_for(db, project, &current).await
}
async fn load_for(
    db: &impl ConnectionTrait,
    project: &str,
    current: &[Credential],
) -> Result<Inventory, ApiError> {
    let mut inventory = Inventory::default();
    let snapshots = db
        .query_all(sql(
            "SELECT * FROM credential_model_snapshots WHERE project_id=?",
            vec![project.into()],
        ))
        .await?;
    for row in snapshots {
        let id: String = row.try_get("", "credential_id")?;
        let fingerprint: String = row.try_get("", "credential_fingerprint")?;
        let config: String = row.try_get("", "provider_config_fingerprint")?;
        let provider: String = row.try_get("", "provider_id")?;
        if !current.iter().any(|c| {
            c.id == id
                && c.provider == provider
                && c.fingerprint == fingerprint
                && c.config_fingerprint == config
        }) {
            continue;
        }
        inventory.0.insert(
            id,
            Snapshot {
                provider,
                status: row.try_get("", "status")?,
                last_success_at: row.try_get("", "last_success_at")?,
                error_code: row.try_get("", "last_error_code")?,
                models: BTreeMap::new(),
            },
        );
    }
    let rows=db.query_all(sql("SELECT a.credential_id,a.upstream_name,a.metadata_json FROM credential_model_availability a JOIN credential_model_snapshots s ON s.credential_id=a.credential_id WHERE s.project_id=?",vec![project.into()])).await?;
    for row in rows {
        let id: String = row.try_get("", "credential_id")?;
        if let Some(snapshot) = inventory.0.get_mut(&id) {
            let metadata: String = row.try_get("", "metadata_json")?;
            snapshot.models.insert(
                row.try_get("", "upstream_name")?,
                StoredModelMetadata::parse(&metadata).and_then(|metadata| metadata.card),
            );
        }
    }
    Ok(inventory)
}

pub async fn sync(state: &AppState, claim: &jobs::Claim, payload: &Value) -> Result<(), ApiError> {
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let selected: Vec<_> = credentials(&state.db, project)
        .await?
        .into_iter()
        .filter(|c| c.provider == provider)
        .collect();
    if selected.is_empty() {
        return Err(ApiError::NotFound);
    }
    let results: Vec<_> = stream::iter(selected)
        .map(|mut credential| async move {
            let result = async {
                if !crate::providers::discovery::credential_adapter_supported(
                    &credential.target.provider_kind,
                    &credential.target.credential_type,
                ) {
                    return Err(ApiError::BadRequest(
                        "model discovery is not available for this credential adapter".into(),
                    ));
                }
                // Revalidate as each bounded work item starts, so queued keys
                // disabled while earlier fetches ran never reach the provider.
                let before = credentials(&state.db, project).await?;
                if !before.iter().any(|current| {
                    current.id == credential.id
                        && current.fingerprint == credential.fingerprint
                        && current.config_fingerprint == credential.config_fingerprint
                }) {
                    return Err(ApiError::NotFound);
                }
                let (secret, refreshed) = crate::api::gateway::resolve_route_credential(
                    state,
                    &credential.target,
                    &credential.provider,
                    &credential.id,
                    project,
                )
                .await?;
                let expected_envelope = refreshed
                    .as_deref()
                    .unwrap_or(&credential.target.secret_envelope);
                let current = credentials(&state.db, project)
                    .await?
                    .into_iter()
                    .find(|current| {
                        current.id == credential.id
                            && current.provider == credential.provider
                            && current.config_fingerprint == credential.config_fingerprint
                            && current.target.secret_envelope == expected_envelope
                            && current.target.credential_type == credential.target.credential_type
                            && (refreshed.is_some()
                                || current.fingerprint == credential.fingerprint)
                    })
                    .ok_or(ApiError::NotFound)?;
                credential = current;
                let client = state.upstream_client(&credential.target).await?;
                crate::providers::discovery::models(
                    &client,
                    &credential.target.provider_kind,
                    &credential.target.base_url,
                    &secret,
                )
                .await
            }
            .await;
            (credential, result)
        })
        .buffer_unordered(4)
        .collect()
        .await;
    let tx = state.db.begin().await?;
    jobs::fence(&tx, claim).await?;
    let current = credentials(&tx, project).await?;
    let mut successes = 0;
    for (captured, result) in results {
        if !current.iter().any(|c| {
            c.id == captured.id
                && c.provider == captured.provider
                && c.fingerprint == captured.fingerprint
                && c.config_fingerprint == captured.config_fingerprint
        }) {
            continue;
        }
        // A stale snapshot may retain availability only for this exact revision.
        let previous=tx.query_one(sql("SELECT last_success_at FROM credential_model_snapshots WHERE credential_id=? AND credential_fingerprint=? AND provider_config_fingerprint=?",vec![captured.id.clone().into(),captured.fingerprint.clone().into(),captured.config_fingerprint.clone().into()])).await?;
        let last_success = previous
            .map(|row| row.try_get::<Option<i64>>("", "last_success_at"))
            .transpose()?
            .flatten();
        let success = result.is_ok();
        let now = db::now();
        tx.execute(sql("INSERT INTO credential_model_snapshots(credential_id,provider_id,project_id,credential_fingerprint,provider_config_fingerprint,status,last_success_at,last_attempt_at,last_error_code) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(credential_id) DO UPDATE SET credential_fingerprint=excluded.credential_fingerprint,provider_config_fingerprint=excluded.provider_config_fingerprint,status=excluded.status,last_success_at=excluded.last_success_at,last_attempt_at=excluded.last_attempt_at,last_error_code=excluded.last_error_code",vec![captured.id.clone().into(),provider.into(),project.into(),captured.fingerprint.into(),captured.config_fingerprint.into(),if success {"known"} else {"stale"}.into(),if success {Some(now)} else {last_success}.into(),now.into(),if success {None} else {Some("model_sync_failed")}.into()])).await?;
        if success || last_success.is_none() {
            tx.execute(sql(
                "DELETE FROM credential_model_availability WHERE credential_id=?",
                vec![captured.id.clone().into()],
            ))
            .await?;
        }
        if let Ok(models) = result {
            successes += 1;
            for model in models {
                tx.execute(sql("INSERT INTO credential_model_availability(credential_id,upstream_name,metadata_json) VALUES(?,?,?)",vec![captured.id.clone().into(),model.id.into(),json!({"card":model.card}).to_string().into()])).await?;
            }
        }
    }
    if successes == 0 {
        super::audit(&tx, None, project, "model.inventory_failed", provider).await?;
        tx.commit().await?;
        return Err(ApiError::Upstream("model discovery failed".into()));
    }
    let inventory = load_for(&tx, project, &current).await?;
    let mut union: BTreeMap<String, Vec<Option<ModelCardProjection>>> = BTreeMap::new();
    for snapshot in inventory
        .0
        .values()
        .filter(|s| s.provider == provider && s.last_success_at.is_some())
    {
        for (model, card) in &snapshot.models {
            union.entry(model.clone()).or_default().push(card.clone());
        }
    }
    let names = json!(union.keys().collect::<Vec<_>>()).to_string();
    tx.execute(sql("UPDATE models SET enabled=0,lifecycle='archived' WHERE provider_id=? AND discovery_managed=1 AND upstream_name NOT IN (SELECT value FROM json_each(?))",vec![provider.into(),names.into()])).await?;
    for (name, cards) in &union {
        if tx.query_one(sql("SELECT id FROM models WHERE provider_id=? AND upstream_name=? AND discovery_managed=0 LIMIT 1",vec![provider.into(),name.clone().into()])).await?.is_some() {
            tx.execute(sql("UPDATE models SET enabled=0,lifecycle='archived' WHERE provider_id=? AND upstream_name=? AND discovery_managed=1",vec![provider.into(),name.clone().into()])).await?;
            continue;
        }
        let digest = blake3::hash(format!("{provider}\0{name}").as_bytes())
            .to_hex()
            .to_string();
        tx.execute(sql("INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at,catalog_metadata_json,lifecycle,discovery_managed) VALUES(?,?,?,?,'[\"chat\"]',0,0,100,1,?,?,'active',1) ON CONFLICT(id) DO UPDATE SET enabled=1,lifecycle='active',catalog_metadata_json=excluded.catalog_metadata_json WHERE models.provider_id=excluded.provider_id AND models.discovery_managed=1",vec![format!("discovered-{}",&digest[..32]).into(),provider.into(),name.clone().into(),name.clone().into(),db::now().into(),json!({"version":1,"card":ModelCardProjection::aggregate(cards)}).to_string().into()])).await?;
    }
    tx.execute(sql("UPDATE channel_settings SET model_sync_error=NULL,model_synced_at=?,model_sync_count=?,updated_at=? WHERE provider_id=?",vec![db::now().into(),i64::try_from(union.len()).unwrap_or(i64::MAX).into(),db::now().into(),provider.into()])).await?;
    super::audit(&tx, None, project, "model.sync", provider).await?;
    tx.commit().await?;
    Ok(())
}

/// Called only inside a successful OAuth refresh CAS transaction. Carry the
/// existing facts to the refreshed envelope of the same enabled credential;
/// this never changes model rows, status, dates or errors. Manual edits do not
/// invoke this helper and therefore invalidate discovery normally.
pub async fn carry_forward_oauth_refresh(
    tx: &impl ConnectionTrait,
    project: &str,
    previous: &Credential,
) -> Result<(), ApiError> {
    let Some(current) = credentials(tx, project)
        .await?
        .into_iter()
        .find(|credential| {
            credential.id == previous.id
                && credential.provider == previous.provider
                && credential.config_fingerprint == previous.config_fingerprint
                && credential.target.credential_type == previous.target.credential_type
        })
    else {
        return Ok(());
    };
    tx.execute(sql("UPDATE credential_model_snapshots SET credential_fingerprint=? WHERE credential_id=? AND provider_id=? AND project_id=? AND credential_fingerprint=? AND provider_config_fingerprint=?",vec![current.fingerprint.into(),previous.id.clone().into(),previous.provider.clone().into(),project.into(),previous.fingerprint.clone().into(),previous.config_fingerprint.clone().into()])).await?;
    Ok(())
}
