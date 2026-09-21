use super::*;
use crate::catalog::{
    self, Catalog, repository as repo,
    types::{MAX_BYTES, invalid},
};
use axum::extract::DefaultBodyLimit;
use serde::Deserialize;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/v1/catalog", get(export))
        .route("/api/admin/v1/catalog/export", get(export))
        .route("/api/admin/v1/catalog/import", post(import))
        .route("/api/admin/v1/catalog/providers", get(providers))
        .route("/api/admin/v1/catalog/providers/{*id}", get(provider))
        .route("/api/admin/v1/catalog/models", get(models))
        .route("/api/admin/v1/catalog/models/{*id}", get(model))
        .route(
            "/api/admin/v1/catalog/overrides/{kind}/{*id}",
            axum::routing::put(override_entry).delete(remove_override),
        )
        .route(
            "/api/admin/v1/catalog/sources",
            get(sources).post(create_source),
        )
        .route(
            "/api/admin/v1/catalog/sources/refresh-due",
            post(refresh_due),
        )
        .route(
            "/api/admin/v1/catalog/sources/{id}",
            axum::routing::put(update_source).delete(delete_source),
        )
        .route("/api/admin/v1/catalog/sources/{id}/refresh", post(refresh))
        .route(
            "/api/admin/v1/catalog/sources/{id}/snapshots",
            get(snapshots),
        )
        .route(
            "/api/admin/v1/catalog/sources/{id}/rollback",
            post(rollback),
        )
        .layer(DefaultBodyLimit::max(MAX_BYTES))
}

async fn manager(state: &AppState, headers: &HeaderMap) -> Result<User, ApiError> {
    let user = require_user(state, headers).await?;
    crate::access::authorize(
        &state.db,
        &crate::access::Principal::session(user.id.clone()),
        None,
        "catalog:manage",
    )
    .await
    .map_err(|error| match error {
        crate::access::AccessError::Internal(error) => ApiError::Internal(error),
        _ => ApiError::Forbidden,
    })?;
    Ok(user)
}
/// Build a catalog audit context for a transactional repository call.
fn audit_ctx<'a>(user: &'a User, action: &'a str) -> repo::CatalogAudit<'a> {
    repo::CatalogAudit {
        actor_user_id: &user.id,
        action,
        resource_id: None,
    }
}

/// Same, but naming the single entry the operation touched.
fn audit_ctx_for<'a>(
    user: &'a User,
    action: &'a str,
    resource_id: &'a str,
) -> repo::CatalogAudit<'a> {
    repo::CatalogAudit {
        actor_user_id: &user.id,
        action,
        resource_id: Some(resource_id),
    }
}

async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Catalog>, ApiError> {
    require_user(&state, &headers).await?;
    Ok(Json(repo::effective(&state.db).await?))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Filter {
    q: Option<String>,
    developer: Option<String>,
    provider: Option<String>,
    #[serde(rename = "type")]
    model_type: Option<String>,
    category: Option<String>,
    modality: Option<String>,
    protocol: Option<String>,
    capability: Option<String>,
    adapter_available: Option<bool>,
    offset: Option<usize>,
    limit: Option<usize>,
}
fn page(values: Vec<Value>, filter: &Filter, version: &str) -> Json<Value> {
    let total = values.len();
    let data = values
        .into_iter()
        .skip(filter.offset.unwrap_or(0))
        .take(filter.limit.unwrap_or(100).clamp(1, 1000))
        .collect::<Vec<_>>();
    Json(json!({"schema_version":1,"version":version,"total":total,"data":data}))
}

async fn providers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    require_user(&state, &headers).await?;
    let catalog = repo::effective(&state.db).await?;
    let q = filter.q.as_deref().unwrap_or("").to_lowercase();
    let data = catalog
        .providers
        .iter()
        .filter(|p| {
            filter.category.as_ref().is_none_or(|v| v == &p.category)
                && filter
                    .adapter_available
                    .is_none_or(|v| v == p.adapter_available)
                && (p.id.to_lowercase().contains(&q) || p.name.to_lowercase().contains(&q))
        })
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::Internal(e.into()))?;
    Ok(page(data, &filter, &catalog.version))
}

async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    require_user(&state, &headers).await?;
    let catalog = repo::effective(&state.db).await?;
    let q = filter.q.as_deref().unwrap_or("").to_lowercase();
    let developer = filter
        .developer
        .as_deref()
        .or(filter.provider.as_deref())
        .map(|value| match value {
            "gemini" | "vertex" | "gcp" => "google",
            "doubao" | "volcengine" => "bytedance",
            "zhipu" => "zai",
            "kimi" => "moonshot",
            "qwen" | "bailian" => "alibaba",
            _ => value,
        });
    let data = catalog
        .models
        .iter()
        .filter(|model| {
            developer.is_none_or(|d| d == model.developer)
                && filter
                    .model_type
                    .as_ref()
                    .is_none_or(|v| v == &model.model_type)
                && filter.modality.as_ref().is_none_or(|v| {
                    model.modalities.input.contains(v) || model.modalities.output.contains(v)
                })
                && filter
                    .protocol
                    .as_ref()
                    .is_none_or(|v| model.protocols.contains(v))
                && filter.capability.as_ref().is_none_or(|v| {
                    serde_json::to_value(&model.capabilities)
                        .is_ok_and(|value| value.get(v).and_then(Value::as_bool) == Some(true))
                })
                && (model.id.to_lowercase().contains(&q)
                    || model.name.to_lowercase().contains(&q)
                    || model.aliases.iter().any(|a| a.to_lowercase().contains(&q)))
        })
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::Internal(e.into()))?;
    Ok(page(data, &filter, &catalog.version))
}

async fn provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<catalog::Provider>, ApiError> {
    require_user(&state, &headers).await?;
    repo::effective(&state.db)
        .await?
        .providers
        .into_iter()
        .find(|p| p.id == id)
        .map(Json)
        .ok_or(ApiError::NotFound)
}
async fn model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<catalog::Model>, ApiError> {
    require_user(&state, &headers).await?;
    repo::effective(&state.db)
        .await?
        .models
        .into_iter()
        .find(|m| m.id == id || m.upstream_id == id || m.aliases.contains(&id))
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
    let user = manager(&state, &headers).await?;
    let (providers, models) =
        repo::import(&state.db, &body, Some(audit_ctx(&user, "import"))).await?;
    Ok(Json(
        json!({"providers":providers,"models":models,"mode":"upsert_local_overrides"}),
    ))
}

async fn override_entry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Json(value): Json<Value>,
) -> Result<StatusCode, ApiError> {
    let user = manager(&state, &headers).await?;
    if value["id"] != id {
        return Err(invalid("override id must match the URL"));
    }
    let mut document = json!({"schema_version":1,"version":"local-admin-override","source":{"id":"local","url":"","revision":"1","license":"local"},"providers":[],"models":[],"extensions":{}});
    match kind.as_str() {
        "provider" => document["providers"] = json!([value]),
        "model" => document["models"] = json!([value]),
        _ => return Err(invalid("override kind must be provider or model")),
    };
    repo::import(
        &state.db,
        document.to_string().as_bytes(),
        Some(audit_ctx_for(&user, "override", &id)),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove_override(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let user = manager(&state, &headers).await?;
    repo::remove_override(
        &state.db,
        &kind,
        &id,
        Some(audit_ctx(&user, "remove_override")),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn sources(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<repo::Source>>, ApiError> {
    manager(&state, &headers).await?;
    Ok(Json(repo::sources(&state.db).await?))
}
async fn create_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<repo::SourceInput>,
) -> Result<(StatusCode, Json<repo::Source>), ApiError> {
    let user = manager(&state, &headers).await?;
    let source =
        repo::create_source(&state.db, input, Some(audit_ctx(&user, "create_source"))).await?;
    Ok((StatusCode::CREATED, Json(source)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceUpdate {
    revision: i64,
    source: repo::SourceInput,
}
async fn update_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<SourceUpdate>,
) -> Result<Json<repo::Source>, ApiError> {
    let user = manager(&state, &headers).await?;
    let source = repo::update_source(
        &state.db,
        &id,
        input.revision,
        input.source,
        Some(audit_ctx(&user, "update_source")),
    )
    .await?;
    Ok(Json(source))
}
async fn delete_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = manager(&state, &headers).await?;
    repo::delete_source(&state.db, &id, Some(audit_ctx(&user, "delete_source"))).await?;
    Ok(StatusCode::NO_CONTENT)
}
/// Refresh fetches catalog data over the network, so the audit cannot share a SQLite
/// transaction with the fetch — it is intentionally recorded after the refresh completes.
async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<catalog::refresh::Outcome>, ApiError> {
    let user = manager(&state, &headers).await?;
    let result = catalog::refresh::refresh(&state.db, &id).await?;
    db::record_audit_event(
        &state.db,
        &user.id,
        "refresh_source",
        "catalog",
        &id,
        json!({}),
    )
    .await?;
    Ok(Json(result))
}
/// Refresh-due runs multiple network fetches; same constraint as refresh above.
async fn refresh_due(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Value>>, ApiError> {
    let user = manager(&state, &headers).await?;
    let mut results = vec![];
    for source in repo::due_sources(&state.db, db::now()).await? {
        match catalog::refresh::refresh(&state.db, &source.id).await {
            Ok(outcome) => results.push(json!(outcome)),
            Err(error) => results.push(
                json!({"source_id":source.id,"status":"failed","error":error.public_message()}),
            ),
        }
    }
    db::record_audit_event(
        &state.db,
        &user.id,
        "refresh_due",
        "catalog",
        "sources",
        json!({}),
    )
    .await?;
    Ok(Json(results))
}
async fn snapshots(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<Value>>, ApiError> {
    manager(&state, &headers).await?;
    Ok(Json(repo::snapshots(&state.db, &id).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rollback {
    revision: i64,
    snapshot_id: String,
}
async fn rollback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Rollback>,
) -> Result<StatusCode, ApiError> {
    let user = manager(&state, &headers).await?;
    repo::rollback(
        &state.db,
        &id,
        &input.snapshot_id,
        input.revision,
        Some(audit_ctx(&user, "rollback_source")),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
