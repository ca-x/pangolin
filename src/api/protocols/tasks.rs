use super::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

fn sql(query: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, query, values)
}

pub(super) async fn get_video(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    execute(state, headers, id, "/v1/videos", false).await
}
pub(super) async fn delete_video(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    execute(state, headers, id, "/v1/videos", true).await
}
pub(super) async fn get_doubao(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    execute(
        state,
        headers,
        id,
        "/doubao/v3/contents/generations/tasks",
        false,
    )
    .await
}
pub(super) async fn delete_doubao(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    execute(
        state,
        headers,
        id,
        "/doubao/v3/contents/generations/tasks",
        true,
    )
    .await
}

async fn execute(
    state: AppState,
    headers: HeaderMap,
    id: String,
    endpoint: &'static str,
    delete: bool,
) -> Result<Response, ApiError> {
    let key = gateway_key(&state, &headers).await?;
    let row=state.db.query_one(sql("SELECT provider_id,credential_id,upstream_model,public_model FROM protocol_tasks WHERE project_id=? AND api_key_id=? AND endpoint=? AND upstream_id=?",vec![key.project_id.into(),key.id.into(),endpoint.into(),id.clone().into()])).await?.ok_or(ApiError::NotFound)?;
    let provider: String = row.try_get("", "provider_id")?;
    let model: String = row.try_get("", "public_model")?;
    gateway::execute_input(
        state,
        headers,
        Input {
            payload: json!({"model":model}),
            endpoint,
            wire: Wire::Task {
                id,
                provider,
                credential: row.try_get("", "credential_id")?,
                upstream_model: row.try_get("", "upstream_model")?,
                delete,
            },
        },
    )
    .await
}

pub(crate) async fn persist(
    state: &AppState,
    key: &crate::models::ApiKeyCredential,
    input: &Input,
    candidate: &crate::orchestration::Candidate,
    model: &str,
    value: Option<&Value>,
) -> Result<(), ApiError> {
    match &input.wire {
        Wire::Task {
            id, delete: true, ..
        } => {
            state.db.execute(sql("DELETE FROM protocol_tasks WHERE project_id=? AND api_key_id=? AND endpoint=? AND upstream_id=?",vec![key.project_id.clone().into(),key.id.clone().into(),input.endpoint.into(),id.clone().into()])).await?;
        }
        Wire::Task { .. } => {}
        _ => {
            let id = value
                .and_then(|v| v.get("id"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= 512)
                .ok_or_else(|| {
                    ApiError::Upstream("task creation response must contain an id".into())
                })?;
            let result=state.db.execute(sql("INSERT INTO protocol_tasks(project_id,api_key_id,endpoint,upstream_id,provider_id,credential_id,upstream_model,public_model,created_at) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(api_key_id,endpoint,upstream_id) DO UPDATE SET public_model=excluded.public_model WHERE provider_id=excluded.provider_id AND credential_id=excluded.credential_id AND upstream_model=excluded.upstream_model",vec![key.project_id.clone().into(),key.id.clone().into(),input.endpoint.into(),id.into(),candidate.provider_id.clone().into(),candidate.credential_id.clone().into(),candidate.target.upstream_name.clone().into(),model.into(),db::now().into()])).await?;
            if result.rows_affected() != 1 {
                return Err(ApiError::Upstream(
                    "upstream task id collides with an existing task".into(),
                ));
            }
        }
    }
    Ok(())
}
