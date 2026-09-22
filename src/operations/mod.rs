//! Authoritative accounting and durable operational work; DuckDB is a projection.
pub mod backup;
pub mod diagnostics;
pub mod instance_backup;
pub mod jobs;
pub mod lifecycle;
pub mod logging;
pub mod pricing;
pub mod proxy;
pub mod runtime;
pub mod schedule;
pub mod settings;
pub mod storage;

use sea_orm::{DbBackend, Statement};
pub(crate) fn sql(query: impl Into<String>, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, query, values)
}
pub(crate) fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub async fn audit(
    db: &impl sea_orm::ConnectionTrait,
    actor: Option<&crate::access::Principal>,
    project: &str,
    action: &str,
    resource: &str,
) -> Result<(), crate::api::ApiError> {
    let kind = match actor.map(|a| &a.kind) {
        Some(crate::access::PrincipalKind::Session) => "session",
        Some(crate::access::PrincipalKind::ApiKey) => "api_key",
        None => "system",
    };
    db.execute(sql("INSERT INTO audit_events(id,actor_user_id,action,resource_type,resource_id,details,created_at) VALUES(?,?,?,'operations',?,?,?)",vec![id().into(),actor.and_then(|a|a.user_id.clone()).into(),action.into(),resource.into(),serde_json::json!({"project_id":project,"principal_kind":kind,"principal_id":actor.map(|a|&a.subject_id)}).to_string().into(),crate::db::now().into()])).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
