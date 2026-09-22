use super::{id, sql};
use crate::{
    api::{ApiError, AppState},
    db,
};
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Export {
    pub version: u32,
    pub generated_at: i64,
    pub runtime: serde_json::Value,
}

pub fn export(state: &AppState) -> Export {
    Export {
        version: 1,
        generated_at: db::now(),
        runtime: state.orchestrator.cache_diagnostics(),
    }
}

pub async fn clear(state: &AppState, actor: &crate::access::Principal) -> Result<Export, ApiError> {
    let before = authoritative_count(state).await?;
    state.orchestrator.clear_diagnostic_caches();
    let after = authoritative_count(state).await?;
    if before != after {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "record-system count changed during cache clear"
        )));
    }
    let tx = state.db.begin().await?;
    tx.execute(sql(
        "INSERT INTO audit_events(id,actor_user_id,action,resource_type,resource_id,details,created_at) VALUES(?,?,'diagnostics.cache.clear','diagnostics','runtime',?,?)",
        vec![
            id().into(),
            actor.user_id.clone().into(),
            serde_json::json!({"principal_kind":"session","authoritative_rows":after}).to_string().into(),
            db::now().into(),
        ],
    ))
    .await?;
    tx.commit().await?;
    Ok(export(state))
}

async fn authoritative_count(state: &AppState) -> Result<i64, ApiError> {
    let row = state
        .db
        .query_one(sql(
            "SELECT (SELECT COUNT(*) FROM providers)+(SELECT COUNT(*) FROM models)+(SELECT COUNT(*) FROM api_keys) AS count",
            vec![],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(row.try_get("", "count")?)
}
