use std::collections::BTreeSet;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{crypto, db};

pub const SYSTEM_MEMBER_ROLE_ID: &str = "00000000-0000-0000-0000-000000000012";

#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("authentication required")]
    Unauthorized,
    #[error("permission denied")]
    Forbidden,
    #[error("resource not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sea_orm::DbErr> for AccessError {
    fn from(value: sea_orm::DbErr) -> Self {
        Self::Internal(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrincipalKind {
    Session,
    ApiKey,
}

#[derive(Debug, Clone)]
pub struct Principal {
    pub kind: PrincipalKind,
    pub subject_id: String,
    pub user_id: Option<String>,
    pub project_id: Option<String>,
    pub scopes: BTreeSet<String>,
}

impl Principal {
    pub fn session(user_id: impl Into<String>) -> Self {
        let user_id = user_id.into();
        Self {
            kind: PrincipalKind::Session,
            subject_id: user_id.clone(),
            user_id: Some(user_id),
            project_id: None,
            scopes: BTreeSet::new(),
        }
    }

    pub fn api_key(
        key_id: impl Into<String>,
        project_id: impl Into<String>,
        user_id: Option<String>,
        scopes: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            kind: PrincipalKind::ApiKey,
            subject_id: key_id.into(),
            user_id,
            project_id: Some(project_id.into()),
            scopes: scopes
                .into_iter()
                .map(|scope| normalize_scope(&scope))
                .collect(),
        }
    }
}

fn normalize_scope(scope: &str) -> String {
    match scope {
        "gateway" => "gateway:use".into(),
        value => value.to_owned(),
    }
}

fn statement(sql: impl Into<String>, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

#[derive(FromQueryResult)]
struct PermissionRow {
    slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromQueryResult)]
pub struct PermissionCatalogEntry {
    pub slug: String,
    pub level: String,
    pub description: String,
}

#[derive(FromQueryResult)]
struct RoleScopeRow {
    project_id: Option<String>,
}

#[derive(FromQueryResult)]
struct ProjectOwnerRow {
    owner_user_id: Option<String>,
}

async fn ensure_can_grant_role(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: Option<&str>,
    role_id: &str,
) -> Result<(), AccessError> {
    let role = RoleScopeRow::find_by_statement(statement(
        "SELECT project_id FROM roles WHERE id=?",
        vec![role_id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)?;
    let valid_scope = match (project_id, role.project_id.as_deref()) {
        (None, None) => true,
        (Some(_), None) => true,
        (Some(project), Some(role_project)) => project == role_project,
        (None, Some(_)) => false,
    };
    if !valid_scope {
        return Err(AccessError::Forbidden);
    }
    let permissions = PermissionRow::find_by_statement(statement(
        "SELECT permission.slug FROM permissions permission JOIN role_permissions assignment ON assignment.permission_id=permission.id WHERE assignment.role_id=?",
        vec![role_id.into()],
    ))
    .all(db)
    .await?;
    for permission in permissions {
        authorize(db, actor, project_id, &permission.slug).await?;
    }
    Ok(())
}

pub async fn effective_scopes(
    db: &DatabaseConnection,
    principal: &Principal,
    project_id: Option<&str>,
) -> Result<BTreeSet<String>, AccessError> {
    if principal.kind == PrincipalKind::ApiKey {
        if project_id.is_none() {
            return Err(AccessError::Forbidden);
        }
        if project_id.is_some_and(|project| principal.project_id.as_deref() != Some(project)) {
            return Err(AccessError::Forbidden);
        }
        return Ok(principal.scopes.clone());
    }
    let user_id = principal
        .user_id
        .as_deref()
        .ok_or(AccessError::Unauthorized)?;
    let rows = if let Some(project_id) = project_id {
        PermissionRow::find_by_statement(statement(
            r#"
            SELECT DISTINCT p.slug
            FROM permissions p
            JOIN role_permissions rp ON rp.permission_id=p.id
            JOIN (
                SELECT role_id FROM user_role_bindings
                WHERE user_id=? AND (
                    project_id IS NULL OR (
                        project_id=? AND EXISTS (
                            SELECT 1 FROM project_memberships membership
                            WHERE membership.user_id=user_role_bindings.user_id
                              AND membership.project_id=user_role_bindings.project_id
                              AND membership.status='active'
                        )
                    )
                )
                UNION
                SELECT role_id FROM project_memberships
                WHERE user_id=? AND project_id=? AND status='active'
            ) assigned ON assigned.role_id=rp.role_id
            "#,
            vec![
                user_id.into(),
                project_id.into(),
                user_id.into(),
                project_id.into(),
            ],
        ))
        .all(db)
        .await?
    } else {
        PermissionRow::find_by_statement(statement(
            "SELECT DISTINCT p.slug FROM permissions p JOIN role_permissions rp ON rp.permission_id=p.id JOIN user_role_bindings b ON b.role_id=rp.role_id WHERE b.user_id=? AND b.project_id IS NULL",
            vec![user_id.into()],
        ))
        .all(db)
        .await?
    };
    Ok(rows.into_iter().map(|row| row.slug).collect())
}

pub async fn authorize(
    db: &DatabaseConnection,
    principal: &Principal,
    project_id: Option<&str>,
    permission: &str,
) -> Result<(), AccessError> {
    let scopes = effective_scopes(db, principal, project_id).await?;
    if permits(&scopes, project_id.is_some(), permission) {
        Ok(())
    } else {
        Err(AccessError::Forbidden)
    }
}

fn permits(scopes: &BTreeSet<String>, project_scoped: bool, permission: &str) -> bool {
    let project_manage = project_scoped
        && matches!(
            permission,
            "project:read" | "role:manage" | "api_key:manage"
        )
        && scopes.contains("project:manage");
    scopes.contains("*") || scopes.contains(permission) || project_manage
}

/// The project permissions this principal may place on a project role.
///
/// Reading the choices requires the same authority as writing a role. The
/// returned rows are filtered with the exact predicate `authorize` uses, so the
/// catalog can explain the write boundary without becoming that boundary.
pub async fn permission_catalog(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<Vec<PermissionCatalogEntry>, AccessError> {
    authorize(db, actor, Some(project_id), "role:manage").await?;
    ProjectOwnerRow::find_by_statement(statement(
        "SELECT owner_user_id FROM projects WHERE id=?",
        vec![project_id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)?;

    let scopes = effective_scopes(db, actor, Some(project_id)).await?;
    let permissions = PermissionCatalogEntry::find_by_statement(statement(
        "SELECT slug,level,description FROM permissions WHERE level='project' ORDER BY slug",
        vec![],
    ))
    .all(db)
    .await?;
    Ok(permissions
        .into_iter()
        .filter(|permission| permits(&scopes, true, &permission.slug))
        .collect())
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub owner_user_id: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct ProjectInput {
    pub name: String,
    pub slug: String,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectUpdate {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub owner_user_id: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct UserView {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub language: String,
    pub theme: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct UserInput {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub language: Option<String>,
    /// Whether the account starts enabled. The console offered this switch and
    /// the backend ignored it, always creating an enabled user.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct UserUpdate {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub enabled: Option<bool>,
    pub password: Option<String>,
    pub current_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct RoleView {
    pub id: String,
    pub project_id: Option<String>,
    pub name: String,
    pub scope: String,
    pub is_system: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub permissions: String,
}

#[derive(Debug, Deserialize)]
pub struct RoleInput {
    pub name: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct MembershipView {
    pub id: String,
    pub project_id: String,
    pub user_id: String,
    /// Identity of the member. A roster that lists only opaque user ids cannot be
    /// read by the project manager who is allowed to see it, and that manager is
    /// not allowed to list users to look the ids up.
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub role_id: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct MembershipInput {
    pub user_id: String,
    pub role_id: String,
    #[serde(default = "active_status")]
    pub status: String,
}

fn active_status() -> String {
    "active".into()
}

#[derive(Debug, Deserialize)]
pub struct InvitationInput {
    pub email: String,
    pub role_id: String,
    pub expires_in_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct InvitationView {
    pub id: String,
    pub project_id: String,
    pub email: String,
    pub role_id: String,
    pub expires_at: i64,
    pub accepted_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct RoleBindingInput {
    pub role_id: String,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct RoleBindingView {
    pub id: String,
    pub user_id: String,
    pub role_id: String,
    pub project_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct AcceptInvitationInput {
    pub token: String,
    pub password: Option<String>,
    pub display_name: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ScopedApiKeyInput {
    pub name: String,
    #[serde(default)]
    pub project_id: String,
    pub user_id: Option<String>,
    pub profile_id: Option<String>,
    #[serde(default = "service_key_type")]
    pub key_type: String,
    #[serde(default = "gateway_scope")]
    pub scopes: Vec<String>,
    pub budget_micros: Option<i64>,
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub denied_ips: Vec<String>,
    #[serde(default)]
    pub token_mode: ApiKeyTokenMode,
    pub token: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyTokenMode {
    #[default]
    Generated,
    ImportExisting,
}

#[derive(Debug, Default, Deserialize)]
pub struct ScopedApiKeyUpdate {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub expires_at: Patch<i64>,
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub profile_id: Patch<String>,
    #[serde(default)]
    pub budget_micros: Patch<i64>,
    pub allowed_ips: Option<Vec<String>>,
    pub denied_ips: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub enum Patch<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

fn service_key_type() -> String {
    "service".into()
}

fn gateway_scope() -> Vec<String> {
    vec!["gateway:use".into()]
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct ScopedApiKeyView {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    #[serde(serialize_with = "serialize_project_scopes")]
    pub scopes: String,
    pub budget_micros: Option<i64>,
    /// How much of the budget has been spent. Without it a key that has run out
    /// of budget is indistinguishable from a working one in the console.
    pub spent_micros: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub project_id: String,
    pub user_id: Option<String>,
    pub profile_id: Option<String>,
    pub key_type: String,
    pub expires_at: Option<i64>,
    /// When the key was last used. Without it a key nobody has used in months is
    /// indistinguishable from one in active use.
    pub last_used_at: Option<i64>,
    pub allowed_ips_json: String,
    pub denied_ips_json: String,
    pub lifecycle: String,
    pub archived_at: Option<i64>,
}

/// SQLite keeps scopes as a JSON array string, while the public API exposes the
/// array itself. A malformed historical row is rendered as no scopes so callers
/// fail closed instead of turning a list response into a serialization failure.
fn serialize_project_scopes<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serde_json::from_str::<Vec<String>>(value)
        .unwrap_or_default()
        .serialize(serializer)
}

/// The fields the key lifecycle decides on, without the secrets or the projections.
#[derive(FromQueryResult)]
struct ApiKeyOwnerRow {
    key_type: String,
    user_id: Option<String>,
    lifecycle: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BulkApiKeyArchiveResult {
    pub archived_ids: Vec<String>,
    pub archived_count: usize,
}

/// Whether this user exists, is enabled and is an active member of this project.
/// The single copy of that predicate: creating an owned key, updating one and the
/// bulk lifecycle all decide on it.
async fn owner_is_active_member<C: ConnectionTrait>(
    db: &C,
    project_id: &str,
    user_id: &str,
) -> Result<bool, AccessError> {
    Ok(PermissionRow::find_by_statement(statement(
        "SELECT 'active' AS slug FROM users user JOIN project_memberships membership ON membership.user_id=user.id AND membership.project_id=? AND membership.status='active' WHERE user.id=? AND user.enabled=1",
        vec![project_id.into(), user_id.into()],
    ))
    .one(db)
    .await?
    .is_some())
}

/// Whether an owned key may be *enabled*. A `user`/`personal` key acts as its owner,
/// so enabling one while the owner is disabled, suspended, missing or no longer an
/// active member of the project would hand out a working credential for an identity
/// that is not allowed in. The single-key update and the bulk lifecycle both call
/// this, so the two cannot drift into different rules.
async fn ensure_owner_can_hold_enabled_key<C: ConnectionTrait>(
    db: &C,
    project_id: &str,
    key_type: &str,
    user_id: Option<&str>,
) -> Result<(), AccessError> {
    if !matches!(key_type, "user" | "personal") {
        return Ok(());
    }
    let Some(owner_id) = user_id else {
        return Err(AccessError::Invalid("owned API key has no owner".into()));
    };
    if !owner_is_active_member(db, project_id, owner_id).await? {
        return Err(AccessError::Invalid(
            "cannot enable a user/personal key without an active owner membership".into(),
        ));
    }
    Ok(())
}

async fn audit<C: ConnectionTrait>(
    db: &C,
    actor: &Principal,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    mut details: Value,
) -> Result<(), AccessError> {
    if actor.kind == PrincipalKind::ApiKey {
        details["actor_api_key_id"] = Value::String(actor.subject_id.clone());
    }
    db.execute(statement(
        "INSERT INTO audit_events(id,actor_user_id,action,resource_type,resource_id,details,created_at) VALUES(?,?,?,?,?,?,?)",
        vec![
            Uuid::new_v4().to_string().into(),
            actor.user_id.clone().into(),
            action.into(),
            resource_type.into(),
            resource_id.into(),
            details.to_string().into(),
            db::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

fn validate_nonempty(value: &str, field: &str) -> Result<String, AccessError> {
    let value = value.trim();
    if value.is_empty() {
        Err(AccessError::Invalid(format!("{field} is required")))
    } else {
        Ok(value.to_owned())
    }
}

fn validate_email(value: &str) -> Result<String, AccessError> {
    let email = value.trim().to_ascii_lowercase();
    if email.split_once('@').is_none_or(|(local, domain)| {
        local.is_empty() || domain.is_empty() || !domain.contains('.')
    }) {
        return Err(AccessError::Invalid("a valid email is required".into()));
    }
    Ok(email)
}

pub async fn list_projects(
    db: &DatabaseConnection,
    actor: &Principal,
) -> Result<Vec<ProjectView>, AccessError> {
    let projects = ProjectView::find_by_statement(statement(
        "SELECT id,name,slug,owner_user_id,is_default,enabled,created_at,updated_at FROM projects ORDER BY name,id",
        vec![],
    ))
    .all(db)
    .await?;
    let mut visible = Vec::new();
    for project in projects {
        if authorize(db, actor, Some(&project.id), "project:read")
            .await
            .is_ok()
        {
            visible.push(project);
        }
    }
    Ok(visible)
}

pub async fn get_project(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<ProjectView, AccessError> {
    authorize(db, actor, Some(project_id), "project:read").await?;
    ProjectView::find_by_statement(statement(
        "SELECT id,name,slug,owner_user_id,is_default,enabled,created_at,updated_at FROM projects WHERE id=?",
        vec![project_id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)
}

pub async fn create_project(
    db: &DatabaseConnection,
    actor: &Principal,
    input: &ProjectInput,
) -> Result<ProjectView, AccessError> {
    authorize(db, actor, None, "project:manage").await?;
    let name = validate_nonempty(&input.name, "name")?;
    let slug = validate_nonempty(&input.slug, "slug")?.to_ascii_lowercase();
    if !slug
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err(AccessError::Invalid(
            "slug contains invalid characters".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let owner = input
        .owner_user_id
        .clone()
        .or_else(|| actor.user_id.clone());
    if let Some(owner_id) = owner.as_deref() {
        ensure_can_grant_role(db, actor, None, db::SYSTEM_OWNER_ROLE_ID).await?;
        if !get_user(db, owner_id).await?.enabled {
            return Err(AccessError::Invalid(
                "project owner must be an active user".into(),
            ));
        }
    }
    let transaction = db.begin().await?;
    transaction
        .execute(statement(
            "INSERT INTO projects(id,name,slug,owner_user_id,is_default,enabled,created_at,updated_at) VALUES(?,?,?,?,0,1,?,?)",
            vec![id.clone().into(), name.clone().into(), slug.clone().into(), owner.clone().into(), timestamp.into(), timestamp.into()],
        ))
        .await
        .map_err(|error| AccessError::Conflict(error.to_string()))?;
    if let Some(owner) = &owner {
        transaction.execute(statement(
            "INSERT INTO project_memberships(id,project_id,user_id,role_id,status,created_at,updated_at) VALUES(?,?,?,?,\'active\',?,?)",
            vec![Uuid::new_v4().to_string().into(), id.clone().into(), owner.clone().into(), crate::db::SYSTEM_OWNER_ROLE_ID.into(), timestamp.into(), timestamp.into()],
        )).await?;
    }
    audit(
        &transaction,
        actor,
        "create",
        "project",
        &id,
        json!({"name":name,"slug":slug}),
    )
    .await?;
    transaction.commit().await?;
    get_project(db, actor, &id).await
}

pub async fn update_project(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    input: &ProjectUpdate,
) -> Result<ProjectView, AccessError> {
    authorize(db, actor, Some(project_id), "project:manage").await?;
    let current = get_project(db, actor, project_id).await?;
    let name = input
        .name
        .as_deref()
        .map(|value| validate_nonempty(value, "name"))
        .transpose()?
        .unwrap_or(current.name);
    let slug = input
        .slug
        .as_deref()
        .map(|value| validate_nonempty(value, "slug"))
        .transpose()?
        .unwrap_or(current.slug);
    let owner = input.owner_user_id.clone().or(current.owner_user_id);
    if let Some(new_owner_id) = input.owner_user_id.as_deref() {
        ensure_can_grant_role(db, actor, Some(project_id), db::SYSTEM_OWNER_ROLE_ID).await?;
        let new_owner = get_user(db, new_owner_id).await?;
        if !new_owner.enabled {
            return Err(AccessError::Invalid(
                "project owner must be an active user".into(),
            ));
        }
    }
    let enabled = input.enabled.unwrap_or(current.enabled);
    let transaction = db.begin().await?;
    transaction
        .execute(statement(
            "UPDATE projects SET name=?,slug=?,owner_user_id=?,enabled=?,updated_at=? WHERE id=?",
            vec![
                name.clone().into(),
                slug.clone().into(),
                owner.clone().into(),
                enabled.into(),
                db::now().into(),
                project_id.into(),
            ],
        ))
        .await
        .map_err(|error| AccessError::Conflict(error.to_string()))?;
    if let Some(new_owner_id) = input.owner_user_id.as_deref() {
        let timestamp = db::now();
        transaction.execute(statement(
            "INSERT INTO project_memberships(id,project_id,user_id,role_id,status,created_at,updated_at) VALUES(?,?,?,?,\'active\',?,?) ON CONFLICT(project_id,user_id) DO UPDATE SET role_id=excluded.role_id,status='active',updated_at=excluded.updated_at",
            vec![Uuid::new_v4().to_string().into(), project_id.into(), new_owner_id.into(), db::SYSTEM_OWNER_ROLE_ID.into(), timestamp.into(), timestamp.into()],
        )).await?;
    }
    audit(
        &transaction,
        actor,
        "update",
        "project",
        project_id,
        json!({"name":name,"slug":slug,"enabled":enabled}),
    )
    .await?;
    transaction.commit().await?;
    get_project(db, actor, project_id).await
}

pub async fn delete_project(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<(), AccessError> {
    authorize(db, actor, Some(project_id), "project:manage").await?;
    if project_id == db::DEFAULT_PROJECT_ID {
        return Err(AccessError::Invalid(
            "the default project cannot be deleted".into(),
        ));
    }
    let transaction = db.begin().await?;
    // Deleting a project cascades into every table that references it. Where
    // immutable history exists — price versions and settled usage — a trigger
    // aborts the statement, which surfaced as an opaque 500 with nothing naming
    // the cause. Refuse with the reason instead.
    let history: i64 = transaction
        .query_one(statement(
            "SELECT (SELECT COUNT(*) FROM model_prices p JOIN models m ON m.id=p.model_id JOIN providers v ON v.id=m.provider_id WHERE v.project_id=?) + (SELECT COUNT(*) FROM usage_logs u JOIN execution_facts e ON e.id=u.execution_id JOIN request_facts f ON f.id=e.request_id WHERE f.project_id=?) AS n",
            vec![project_id.into(), project_id.into()],
        ))
        .await?
        .and_then(|row| row.try_get::<i64>("", "n").ok())
        .unwrap_or(0);
    if history > 0 {
        return Err(AccessError::Invalid(
            "this project has price or usage history and cannot be deleted; disable it instead"
                .into(),
        ));
    }
    audit(
        &transaction,
        actor,
        "delete",
        "project",
        project_id,
        json!({}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM projects WHERE id=?",
            vec![project_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn list_users(
    db: &DatabaseConnection,
    actor: &Principal,
) -> Result<Vec<UserView>, AccessError> {
    authorize(db, actor, None, "user:manage").await?;
    Ok(UserView::find_by_statement(statement(
        "SELECT id,email,display_name,avatar_url,role,language,theme,enabled,created_at,updated_at FROM users ORDER BY email",
        vec![],
    )).all(db).await?)
}

pub async fn create_user(
    db: &DatabaseConnection,
    actor: &Principal,
    input: &UserInput,
) -> Result<UserView, AccessError> {
    authorize(db, actor, None, "user:manage").await?;
    let email = validate_email(&input.email)?;
    if input.password.len() < 12 {
        return Err(AccessError::Invalid(
            "password must contain at least 12 characters".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let display_name = input.display_name.as_deref().unwrap_or(&email).trim();
    let password_hash = crypto::hash_password(&input.password).map_err(AccessError::Internal)?;
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO users(id,email,password_hash,role,language,theme,created_at,display_name,enabled,updated_at) VALUES(?,?,?,'member',?,'system:bronze',?,?,?,?)",
        vec![id.clone().into(), email.clone().into(), password_hash.into(), input.language.clone().unwrap_or_else(|| "zh-CN".into()).into(), timestamp.into(), display_name.into(), input.enabled.into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    audit(
        &transaction,
        actor,
        "create",
        "user",
        &id,
        json!({"email":email}),
    )
    .await?;
    transaction.commit().await?;
    get_user(db, &id).await
}

pub async fn get_user(db: &DatabaseConnection, id: &str) -> Result<UserView, AccessError> {
    UserView::find_by_statement(statement(
        "SELECT id,email,display_name,avatar_url,role,language,theme,enabled,created_at,updated_at FROM users WHERE id=?",
        vec![id.into()],
    )).one(db).await?.ok_or(AccessError::NotFound)
}

pub async fn update_user(
    db: &DatabaseConnection,
    actor: &Principal,
    user_id: &str,
    input: &UserUpdate,
) -> Result<UserView, AccessError> {
    let self_update =
        actor.kind == PrincipalKind::Session && actor.user_id.as_deref() == Some(user_id);
    if !self_update {
        authorize(db, actor, None, "user:manage").await?;
    }
    let current = get_user(db, user_id).await?;
    if input.enabled.is_some() && !matches!(authorize(db, actor, None, "user:manage").await, Ok(()))
    {
        return Err(AccessError::Forbidden);
    }
    if self_update && input.enabled == Some(false) {
        return Err(AccessError::Invalid(
            "cannot deactivate the current user".into(),
        ));
    }
    if let Some(password) = &input.password
        && password.len() < 12
    {
        return Err(AccessError::Invalid(
            "password must contain at least 12 characters".into(),
        ));
    }
    if self_update && input.password.is_some() {
        #[derive(FromQueryResult)]
        struct PasswordRow {
            password_hash: String,
        }
        let password = PasswordRow::find_by_statement(statement(
            "SELECT password_hash FROM users WHERE id=?",
            vec![user_id.into()],
        ))
        .one(db)
        .await?
        .ok_or(AccessError::NotFound)?;
        if input
            .current_password
            .as_deref()
            .is_none_or(|candidate| !crypto::verify_password(candidate, &password.password_hash))
        {
            return Err(AccessError::Forbidden);
        }
    }
    let password_hash = input
        .password
        .as_ref()
        .map(|password| crypto::hash_password(password))
        .transpose()
        .map_err(AccessError::Internal)?;
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "UPDATE users SET display_name=?,avatar_url=?,language=?,theme=?,enabled=?,password_hash=COALESCE(?,password_hash),updated_at=? WHERE id=?",
        vec![input.display_name.clone().unwrap_or(current.display_name).into(), input.avatar_url.clone().or(current.avatar_url).into(), input.language.clone().unwrap_or(current.language).into(), input.theme.clone().unwrap_or(current.theme).into(), input.enabled.unwrap_or(current.enabled).into(), password_hash.into(), db::now().into(), user_id.into()],
    )).await?;
    if input.enabled == Some(false) {
        transaction
            .execute(statement(
                "UPDATE api_keys SET enabled=0 WHERE user_id=? AND key_type IN ('user','personal')",
                vec![user_id.into()],
            ))
            .await?;
    }
    if input.password.is_some() || input.enabled == Some(false) {
        transaction
            .execute(statement(
                "DELETE FROM sessions WHERE user_id=?",
                vec![user_id.into()],
            ))
            .await?;
    }
    audit(
        &transaction,
        actor,
        "update",
        "user",
        user_id,
        json!({"password_changed":input.password.is_some()}),
    )
    .await?;
    transaction.commit().await?;
    get_user(db, user_id).await
}

pub async fn delete_user(
    db: &DatabaseConnection,
    actor: &Principal,
    user_id: &str,
) -> Result<(), AccessError> {
    authorize(db, actor, None, "user:manage").await?;
    if actor.user_id.as_deref() == Some(user_id) {
        return Err(AccessError::Invalid(
            "cannot delete the current user".into(),
        ));
    }
    let transaction = db.begin().await?;
    audit(&transaction, actor, "delete", "user", user_id, json!({})).await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM users WHERE id=?",
            vec![user_id.into()],
        ))
        .await
        .map_err(|error| AccessError::Conflict(error.to_string()))?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn list_roles(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<Vec<RoleView>, AccessError> {
    authorize(db, actor, Some(project_id), "project:read").await?;
    Ok(RoleView::find_by_statement(statement(
        "SELECT id,project_id,name,scope,is_system,created_at,updated_at,COALESCE((SELECT json_group_array(permission.slug) FROM role_permissions assignment JOIN permissions permission ON permission.id=assignment.permission_id WHERE assignment.role_id=roles.id),'[]') AS permissions FROM roles WHERE project_id IS NULL OR project_id=? ORDER BY is_system DESC,name",
        vec![project_id.into()],
    )).all(db).await?)
}

pub async fn create_role(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    input: &RoleInput,
) -> Result<RoleView, AccessError> {
    authorize(db, actor, Some(project_id), "role:manage").await?;
    let name = validate_nonempty(&input.name, "name")?;
    for permission in &input.permissions {
        authorize(db, actor, Some(project_id), permission).await?;
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO roles(id,project_id,name,scope,is_system,created_at,updated_at) VALUES(?,? ,?,'project',0,?,?)",
        vec![id.clone().into(), project_id.into(), name.clone().into(), timestamp.into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    for permission in &input.permissions {
        let result = transaction.execute(statement(
            "INSERT INTO role_permissions(role_id,permission_id,created_at) SELECT ?,id,? FROM permissions WHERE slug=? AND level='project'",
            vec![id.clone().into(), timestamp.into(), permission.into()],
        )).await?;
        if result.rows_affected() == 0 {
            return Err(AccessError::Invalid(format!(
                "unknown project permission `{permission}`"
            )));
        }
    }
    audit(
        &transaction,
        actor,
        "create",
        "role",
        &id,
        json!({"project_id":project_id,"name":name,"permissions":input.permissions}),
    )
    .await?;
    transaction.commit().await?;
    RoleView::find_by_statement(statement(
        "SELECT id,project_id,name,scope,is_system,created_at,updated_at,COALESCE((SELECT json_group_array(permission.slug) FROM role_permissions assignment JOIN permissions permission ON permission.id=assignment.permission_id WHERE assignment.role_id=roles.id),'[]') AS permissions FROM roles WHERE id=?",
        vec![id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)
}

pub async fn update_role(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    role_id: &str,
    input: &RoleInput,
) -> Result<RoleView, AccessError> {
    authorize(db, actor, Some(project_id), "role:manage").await?;
    let name = validate_nonempty(&input.name, "name")?;
    for permission in &input.permissions {
        authorize(db, actor, Some(project_id), permission).await?;
    }
    let transaction = db.begin().await?;
    let result = transaction
        .execute(statement(
            "UPDATE roles SET name=?,updated_at=? WHERE id=? AND project_id=? AND is_system=0",
            vec![
                name.clone().into(),
                db::now().into(),
                role_id.into(),
                project_id.into(),
            ],
        ))
        .await
        .map_err(|error| AccessError::Conflict(error.to_string()))?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction
        .execute(statement(
            "DELETE FROM role_permissions WHERE role_id=?",
            vec![role_id.into()],
        ))
        .await?;
    for permission in &input.permissions {
        let result = transaction.execute(statement("INSERT INTO role_permissions(role_id,permission_id,created_at) SELECT ?,id,? FROM permissions WHERE slug=? AND level='project'", vec![role_id.into(), db::now().into(), permission.into()])).await?;
        if result.rows_affected() == 0 {
            return Err(AccessError::Invalid(format!(
                "unknown project permission `{permission}`"
            )));
        }
    }
    audit(
        &transaction,
        actor,
        "update",
        "role",
        role_id,
        json!({"project_id":project_id,"name":name,"permissions":input.permissions}),
    )
    .await?;
    transaction.commit().await?;
    RoleView::find_by_statement(statement(
        "SELECT id,project_id,name,scope,is_system,created_at,updated_at,COALESCE((SELECT json_group_array(permission.slug) FROM role_permissions assignment JOIN permissions permission ON permission.id=assignment.permission_id WHERE assignment.role_id=roles.id),'[]') AS permissions FROM roles WHERE id=?",
        vec![role_id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)
}

pub async fn delete_role(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    role_id: &str,
) -> Result<(), AccessError> {
    authorize(db, actor, Some(project_id), "role:manage").await?;
    let transaction = db.begin().await?;
    audit(
        &transaction,
        actor,
        "delete",
        "role",
        role_id,
        json!({"project_id":project_id}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM roles WHERE id=? AND project_id=? AND is_system=0",
            vec![role_id.into(), project_id.into()],
        ))
        .await
        .map_err(|error| AccessError::Conflict(error.to_string()))?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn list_memberships(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<Vec<MembershipView>, AccessError> {
    authorize(db, actor, Some(project_id), "project:read").await?;
    Ok(MembershipView::find_by_statement(statement("SELECT m.id AS id,m.project_id AS project_id,m.user_id AS user_id,u.email AS email,u.display_name AS display_name,m.role_id AS role_id,m.status AS status,m.created_at AS created_at,m.updated_at AS updated_at FROM project_memberships m LEFT JOIN users u ON u.id=m.user_id WHERE m.project_id=? ORDER BY m.created_at,m.id", vec![project_id.into()])).all(db).await?)
}

pub async fn upsert_membership(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    input: &MembershipInput,
) -> Result<MembershipView, AccessError> {
    authorize(db, actor, Some(project_id), "project:manage").await?;
    ensure_can_grant_role(db, actor, Some(project_id), &input.role_id).await?;
    if !matches!(input.status.as_str(), "active" | "suspended") {
        return Err(AccessError::Invalid(
            "status must be active or suspended".into(),
        ));
    }
    let project = get_project(db, actor, project_id).await?;
    if project.owner_user_id.as_deref() == Some(&input.user_id)
        && (input.status != "active" || input.role_id != db::SYSTEM_OWNER_ROLE_ID)
    {
        return Err(AccessError::Invalid(
            "project owner membership must remain active with the owner role".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO project_memberships(id,project_id,user_id,role_id,status,created_at,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(project_id,user_id) DO UPDATE SET role_id=excluded.role_id,status=excluded.status,updated_at=excluded.updated_at",
        vec![id.into(), project_id.into(), input.user_id.clone().into(), input.role_id.clone().into(), input.status.clone().into(), timestamp.into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Invalid(error.to_string()))?;
    if input.status == "suspended" {
        transaction.execute(statement("UPDATE api_keys SET enabled=0 WHERE project_id=? AND user_id=? AND key_type IN ('user','personal')", vec![project_id.into(), input.user_id.clone().into()])).await?;
    }
    audit(
        &transaction,
        actor,
        "upsert",
        "project_membership",
        &input.user_id,
        json!({"project_id":project_id,"role_id":input.role_id,"status":input.status}),
    )
    .await?;
    transaction.commit().await?;
    MembershipView::find_by_statement(statement("SELECT m.id AS id,m.project_id AS project_id,m.user_id AS user_id,u.email AS email,u.display_name AS display_name,m.role_id AS role_id,m.status AS status,m.created_at AS created_at,m.updated_at AS updated_at FROM project_memberships m LEFT JOIN users u ON u.id=m.user_id WHERE m.project_id=? AND m.user_id=?", vec![project_id.into(), input.user_id.clone().into()])).one(db).await?.ok_or(AccessError::NotFound)
}

pub async fn remove_membership(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    user_id: &str,
) -> Result<(), AccessError> {
    authorize(db, actor, Some(project_id), "project:manage").await?;
    let transaction = db.begin().await?;
    let owner = ProjectView::find_by_statement(statement("SELECT id,name,slug,owner_user_id,is_default,enabled,created_at,updated_at FROM projects WHERE id=?", vec![project_id.into()])).one(&transaction).await?.ok_or(AccessError::NotFound)?;
    if owner.owner_user_id.as_deref() == Some(user_id) {
        return Err(AccessError::Invalid(
            "project owner membership cannot be removed".into(),
        ));
    }
    audit(
        &transaction,
        actor,
        "delete",
        "project_membership",
        user_id,
        json!({"project_id":project_id}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM project_memberships WHERE project_id=? AND user_id=?",
            vec![project_id.into(), user_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.execute(statement("UPDATE api_keys SET enabled=0 WHERE project_id=? AND user_id=? AND key_type IN ('user','personal')", vec![project_id.into(), user_id.into()])).await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn create_invitation(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    input: &InvitationInput,
) -> Result<(InvitationView, String), AccessError> {
    authorize(db, actor, Some(project_id), "project:manage").await?;
    ensure_can_grant_role(db, actor, Some(project_id), &input.role_id).await?;
    let email = validate_email(&input.email)?;
    let ttl = input.expires_in_seconds.unwrap_or(7 * 24 * 3600);
    if !(60..=30 * 24 * 3600).contains(&ttl) {
        return Err(AccessError::Invalid(
            "invitation lifetime must be between 60 seconds and 30 days".into(),
        ));
    }
    let token = crypto::opaque_token("pi_");
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO project_invitations(id,project_id,email,role_id,invited_by_user_id,token_hash,expires_at,created_at) VALUES(?,?,?,?,?,?,?,?)",
        vec![id.clone().into(), project_id.into(), email.clone().into(), input.role_id.clone().into(), actor.user_id.clone().into(), crypto::token_hash(&token).into(), (timestamp + ttl).into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Invalid(error.to_string()))?;
    audit(
        &transaction,
        actor,
        "create",
        "project_invitation",
        &id,
        json!({"project_id":project_id,"email":email,"role_id":input.role_id}),
    )
    .await?;
    transaction.commit().await?;
    let invitation = inspect_invitation(db, &token).await?;
    Ok((invitation, token))
}

pub async fn list_invitations(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<Vec<InvitationView>, AccessError> {
    authorize(db, actor, Some(project_id), "project:manage").await?;
    Ok(InvitationView::find_by_statement(statement("SELECT id,project_id,email,role_id,expires_at,accepted_at,created_at FROM project_invitations WHERE project_id=? ORDER BY created_at DESC,id", vec![project_id.into()])).all(db).await?)
}

pub async fn delete_invitation(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    invitation_id: &str,
) -> Result<(), AccessError> {
    authorize(db, actor, Some(project_id), "project:manage").await?;
    let transaction = db.begin().await?;
    audit(
        &transaction,
        actor,
        "delete",
        "project_invitation",
        invitation_id,
        json!({"project_id":project_id}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM project_invitations WHERE id=? AND project_id=?",
            vec![invitation_id.into(), project_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn inspect_invitation(
    db: &DatabaseConnection,
    token: &str,
) -> Result<InvitationView, AccessError> {
    InvitationView::find_by_statement(statement(
        "SELECT id,project_id,email,role_id,expires_at,accepted_at,created_at FROM project_invitations WHERE token_hash=? AND accepted_at IS NULL AND expires_at>?",
        vec![crypto::token_hash(token).into(), db::now().into()],
    )).one(db).await?.ok_or(AccessError::NotFound)
}

pub async fn accept_invitation(
    db: &DatabaseConnection,
    authenticated_principal: Option<&Principal>,
    input: &AcceptInvitationInput,
) -> Result<UserView, AccessError> {
    let invitation = inspect_invitation(db, &input.token).await?;
    #[derive(FromQueryResult)]
    struct Existing {
        id: String,
    }
    let existing = Existing::find_by_statement(statement(
        "SELECT id FROM users WHERE email=? COLLATE NOCASE",
        vec![invitation.email.clone().into()],
    ))
    .one(db)
    .await?;
    let timestamp = db::now();
    let transaction = db.begin().await?;
    let user_id = if let Some(existing) = existing {
        if authenticated_principal.is_none_or(|principal| {
            principal.kind != PrincipalKind::Session
                || principal.user_id.as_deref() != Some(existing.id.as_str())
        }) {
            return Err(AccessError::Forbidden);
        }
        existing.id
    } else {
        if authenticated_principal.is_some() {
            return Err(AccessError::Forbidden);
        }
        let password = input
            .password
            .as_deref()
            .ok_or_else(|| AccessError::Invalid("password is required for a new user".into()))?;
        if password.len() < 12 {
            return Err(AccessError::Invalid(
                "password must contain at least 12 characters".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        transaction.execute(statement(
            "INSERT INTO users(id,email,password_hash,role,language,theme,created_at,display_name,enabled,updated_at) VALUES(?,?,?,'member',?,'system:bronze',?,?,1,?)",
            vec![id.clone().into(), invitation.email.clone().into(), crypto::hash_password(password).map_err(AccessError::Internal)?.into(), input.language.clone().unwrap_or_else(|| "zh-CN".into()).into(), timestamp.into(), input.display_name.clone().unwrap_or_else(|| invitation.email.clone()).into(), timestamp.into()],
        )).await?;
        id
    };
    let claimed = transaction.execute(statement("UPDATE project_invitations SET accepted_at=? WHERE id=? AND accepted_at IS NULL AND expires_at>?", vec![timestamp.into(), invitation.id.clone().into(), timestamp.into()])).await?;
    if claimed.rows_affected() == 0 {
        return Err(AccessError::Conflict(
            "invitation was already used or expired".into(),
        ));
    }
    let project_owner = ProjectOwnerRow::find_by_statement(statement(
        "SELECT owner_user_id FROM projects WHERE id=?",
        vec![invitation.project_id.clone().into()],
    ))
    .one(&transaction)
    .await?
    .ok_or(AccessError::NotFound)?;
    if project_owner.owner_user_id.as_deref() == Some(&user_id)
        && invitation.role_id != db::SYSTEM_OWNER_ROLE_ID
    {
        return Err(AccessError::Invalid(
            "project owner membership must retain the owner role".into(),
        ));
    }
    transaction.execute(statement(
        "INSERT INTO project_memberships(id,project_id,user_id,role_id,status,created_at,updated_at) VALUES(?,?,?,?,\'active\',?,?) ON CONFLICT(project_id,user_id) DO UPDATE SET role_id=excluded.role_id,status='active',updated_at=excluded.updated_at",
        vec![Uuid::new_v4().to_string().into(), invitation.project_id.clone().into(), user_id.clone().into(), invitation.role_id.clone().into(), timestamp.into(), timestamp.into()],
    )).await?;
    let actor = Principal::session(user_id.clone());
    audit(
        &transaction,
        &actor,
        "accept",
        "project_invitation",
        &invitation.id,
        json!({"project_id":invitation.project_id}),
    )
    .await?;
    transaction.commit().await?;
    get_user(db, &user_id).await
}

pub async fn create_scoped_api_key(
    db: &DatabaseConnection,
    actor: &Principal,
    input: &ScopedApiKeyInput,
) -> Result<(ScopedApiKeyView, String), AccessError> {
    authorize(db, actor, Some(&input.project_id), "api_key:manage").await?;
    let name = validate_nonempty(&input.name, "name")?;
    if !matches!(
        input.key_type.as_str(),
        "user" | "service" | "personal" | "no_auth"
    ) {
        return Err(AccessError::Invalid("invalid API-key type".into()));
    }
    if matches!(input.key_type.as_str(), "user" | "personal") && input.user_id.is_none() {
        return Err(AccessError::Invalid(
            "this API-key type requires a user owner".into(),
        ));
    }
    if matches!(input.key_type.as_str(), "user" | "personal")
        && let Some(user_id) = &input.user_id
        && !owner_is_active_member(db, &input.project_id, user_id).await?
    {
        return Err(AccessError::Invalid(
            "user/personal API-key owner must be an active user and project member".into(),
        ));
    }
    if input.expires_at.is_some_and(|expires| expires <= db::now()) {
        return Err(AccessError::Invalid(
            "expiration must be in the future".into(),
        ));
    }
    let scopes = input
        .scopes
        .iter()
        .map(|value| normalize_scope(value))
        .collect::<Vec<_>>();
    if scopes.is_empty() {
        return Err(AccessError::Invalid(
            "at least one scope is required".into(),
        ));
    }
    for scope in &scopes {
        let known = PermissionRow::find_by_statement(statement(
            "SELECT slug FROM permissions WHERE slug=? AND level='project'",
            vec![scope.clone().into()],
        ))
        .one(db)
        .await?
        .is_some();
        if !known {
            return Err(AccessError::Invalid(format!(
                "unknown project scope `{scope}`"
            )));
        }
        authorize(db, actor, Some(&input.project_id), scope).await?;
    }
    let token = match input.token_mode {
        ApiKeyTokenMode::Generated => {
            if input.token.is_some() {
                return Err(AccessError::Invalid(
                    "token is only accepted in import_existing mode".into(),
                ));
            }
            crypto::opaque_token("pg_")
        }
        ApiKeyTokenMode::ImportExisting => {
            let token = input
                .token
                .as_deref()
                .ok_or_else(|| AccessError::Invalid("token is required for import".into()))?;
            validate_imported_token(token)?;
            token.to_owned()
        }
    };
    let lookup_digest = crypto::token_hash(&token);
    let prefix = lookup_digest[..8].to_owned();
    let id = Uuid::new_v4().to_string();
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,allowed_ips_json,denied_ips_json) VALUES(?,?,?,?,?,?,?,0,1,?,?,?,?,?,?,?,?)",
        vec![id.clone().into(), name.clone().into(), prefix.clone().into(), crypto::hash_password(&token).map_err(AccessError::Internal)?.into(), lookup_digest.into(), serde_json::to_string(&scopes).map_err(|e| AccessError::Internal(e.into()))?.into(), input.budget_micros.into(), db::now().into(), input.project_id.clone().into(), input.user_id.clone().into(), input.profile_id.clone().into(), input.key_type.clone().into(), input.expires_at.into(), serde_json::to_string(&input.allowed_ips).map_err(|e| AccessError::Internal(e.into()))?.into(), serde_json::to_string(&input.denied_ips).map_err(|e| AccessError::Internal(e.into()))?.into()],
    )).await.map_err(|error| {
        if error.to_string().contains("lookup_digest") {
            AccessError::Conflict("API token is already registered".into())
        } else {
            AccessError::Invalid("API key could not be created".into())
        }
    })?;
    audit(&transaction, actor, "create", "api_key", &id, json!({"project_id":input.project_id,"name":name,"fingerprint":prefix,"key_type":input.key_type,"token_mode":match input.token_mode { ApiKeyTokenMode::Generated => "generated", ApiKeyTokenMode::ImportExisting => "import_existing" }})).await?;
    transaction.commit().await?;
    let view = ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,last_used_at,allowed_ips_json,denied_ips_json,lifecycle,archived_at FROM api_keys WHERE id=?", vec![id.into()])).one(db).await?.ok_or(AccessError::NotFound)?;
    Ok((view, token))
}

fn validate_imported_token(token: &str) -> Result<(), AccessError> {
    if !crypto::imported_token_has_entropy(token) {
        return Err(AccessError::Invalid(
            "imported token must contain 32–1024 high-entropy, non-whitespace characters".into(),
        ));
    }
    Ok(())
}

pub async fn list_scoped_api_keys(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<Vec<ScopedApiKeyView>, AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    Ok(ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,last_used_at,allowed_ips_json,denied_ips_json,lifecycle,archived_at FROM api_keys WHERE project_id=? ORDER BY created_at DESC,id", vec![project_id.into()])).all(db).await?)
}

pub async fn update_scoped_api_key(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    key_id: &str,
    input: &ScopedApiKeyUpdate,
) -> Result<ScopedApiKeyView, AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    if matches!(input.expires_at,Patch::Value(expires) if expires <= db::now()) {
        return Err(AccessError::Invalid(
            "expiration must be in the future".into(),
        ));
    }
    let current = ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,last_used_at,allowed_ips_json,denied_ips_json,lifecycle,archived_at FROM api_keys WHERE id=? AND project_id=?", vec![key_id.into(), project_id.into()])).one(db).await?.ok_or(AccessError::NotFound)?;
    if current.lifecycle == "archived" {
        return Err(AccessError::Conflict(
            "archived API keys cannot be modified".into(),
        ));
    }
    if input.enabled == Some(true) {
        ensure_owner_can_hold_enabled_key(
            db,
            project_id,
            &current.key_type,
            current.user_id.as_deref(),
        )
        .await?;
    }
    let scopes = if let Some(scopes) = &input.scopes {
        let scopes = scopes
            .iter()
            .map(|scope| normalize_scope(scope))
            .collect::<Vec<_>>();
        if scopes.is_empty() {
            return Err(AccessError::Invalid(
                "at least one scope is required".into(),
            ));
        }
        for scope in &scopes {
            let known = PermissionRow::find_by_statement(statement(
                "SELECT slug FROM permissions WHERE slug=? AND level='project'",
                vec![scope.clone().into()],
            ))
            .one(db)
            .await?
            .is_some();
            if !known {
                return Err(AccessError::Invalid(format!(
                    "unknown project scope `{scope}`"
                )));
            }
            authorize(db, actor, Some(project_id), scope).await?;
        }
        serde_json::to_string(&scopes).map_err(|error| AccessError::Internal(error.into()))?
    } else {
        current.scopes.clone()
    };
    let name = input
        .name
        .as_deref()
        .map(|name| validate_nonempty(name, "name"))
        .transpose()?
        .unwrap_or(current.name);
    if matches!(input.budget_micros,Patch::Value(value) if value < 0) {
        return Err(AccessError::Invalid("budget must be non-negative".into()));
    }
    if let Patch::Value(profile) = &input.profile_id
        && !profile.is_empty()
        && PermissionRow::find_by_statement(statement(
            "SELECT 'profile' AS slug FROM api_key_profiles WHERE id=? AND project_id=?",
            vec![profile.into(), project_id.into()],
        ))
        .one(db)
        .await?
        .is_none()
    {
        return Err(AccessError::Invalid(
            "profile does not belong to the project".into(),
        ));
    }
    for rule in input
        .allowed_ips
        .iter()
        .chain(input.denied_ips.iter())
        .flatten()
    {
        if rule.parse::<ipnet::IpNet>().is_err() && rule.parse::<std::net::IpAddr>().is_err() {
            return Err(AccessError::Invalid(format!("invalid IP policy `{rule}`")));
        }
    }
    let current_allowed: Vec<String> =
        serde_json::from_str(&current.allowed_ips_json).unwrap_or_default();
    let current_denied: Vec<String> =
        serde_json::from_str(&current.denied_ips_json).unwrap_or_default();
    let allowed = serde_json::to_string(input.allowed_ips.as_ref().unwrap_or(&current_allowed))
        .map_err(|error| AccessError::Internal(error.into()))?;
    let denied = serde_json::to_string(input.denied_ips.as_ref().unwrap_or(&current_denied))
        .map_err(|error| AccessError::Internal(error.into()))?;
    let transaction = db.begin().await?;
    let (expires_set, expires_at) = match &input.expires_at {
        Patch::Missing => (false, current.expires_at),
        Patch::Null => (true, None),
        Patch::Value(value) => (true, Some(*value)),
    };
    let (profile_set, profile_id) = match &input.profile_id {
        Patch::Missing => (false, current.profile_id.clone()),
        Patch::Null => (true, None),
        Patch::Value(value) => (true, Some(value.clone())),
    };
    let (budget_set, budget) = match &input.budget_micros {
        Patch::Missing => (false, current.budget_micros),
        Patch::Null => (true, None),
        Patch::Value(value) => (true, Some(*value)),
    };
    transaction.execute(statement("UPDATE api_keys SET name=?,enabled=?,expires_at=CASE WHEN ? THEN ? ELSE expires_at END,scopes=?,profile_id=CASE WHEN ? THEN ? ELSE profile_id END,budget_micros=CASE WHEN ? THEN ? ELSE budget_micros END,allowed_ips_json=?,denied_ips_json=? WHERE id=? AND project_id=?", vec![name.clone().into(), input.enabled.unwrap_or(current.enabled).into(), expires_set.into(),expires_at.into(), scopes.into(),profile_set.into(),profile_id.into(),budget_set.into(),budget.into(), allowed.into(), denied.into(), key_id.into(), project_id.into()])).await?;
    audit(
        &transaction,
        actor,
        "update",
        "api_key",
        key_id,
        json!({"project_id":project_id,"name":name,"enabled":input.enabled,"profile_changed":profile_set,"budget_changed":budget_set,"expiration_changed":expires_set,"ip_policy_changed":input.allowed_ips.is_some()||input.denied_ips.is_some()}),
    )
    .await?;
    transaction.commit().await?;
    ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,last_used_at,allowed_ips_json,denied_ips_json,lifecycle,archived_at FROM api_keys WHERE id=? AND project_id=?", vec![key_id.into(), project_id.into()])).one(db).await?.ok_or(AccessError::NotFound)
}

pub async fn rotate_scoped_api_key(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    key_id: &str,
) -> Result<(ScopedApiKeyView, String), AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    // Argon2id is intentionally outside the SQLite business transaction. A refused
    // rotate may waste one hash, but never holds the writer while doing expensive CPU.
    let token = crypto::opaque_token("pg_");
    let lookup_digest = crypto::token_hash(&token);
    let prefix = lookup_digest[..8].to_owned();
    let key_hash = crypto::hash_password(&token).map_err(AccessError::Internal)?;
    let transaction = db.begin().await?;
    let current = ApiKeyOwnerRow::find_by_statement(statement(
        "SELECT key_type,user_id,lifecycle FROM api_keys WHERE id=? AND project_id=?",
        vec![key_id.into(), project_id.into()],
    ))
    .one(&transaction)
    .await?
    .ok_or(AccessError::NotFound)?;
    if current.lifecycle == "archived" {
        return Err(AccessError::Conflict(
            "archived API keys cannot be rotated".into(),
        ));
    }
    ensure_owner_can_hold_enabled_key(
        &transaction,
        project_id,
        &current.key_type,
        current.user_id.as_deref(),
    )
    .await?;
    let changed = transaction
        .execute(statement(
            "UPDATE api_keys SET key_prefix=?,key_hash=?,lookup_digest=? WHERE id=? AND project_id=? AND lifecycle='active'",
            vec![prefix.clone().into(), key_hash.into(), lookup_digest.into(), key_id.into(), project_id.into()],
        ))
        .await?;
    if changed.rows_affected() != 1 {
        return Err(AccessError::NotFound);
    }
    audit(
        &transaction,
        actor,
        "rotate",
        "api_key",
        key_id,
        json!({"project_id":project_id,"fingerprint":prefix}),
    )
    .await?;
    transaction.commit().await?;
    let view = ScopedApiKeyView::find_by_statement(statement(
        "SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,last_used_at,allowed_ips_json,denied_ips_json,lifecycle,archived_at FROM api_keys WHERE id=? AND project_id=?",
        vec![key_id.into(), project_id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)?;
    Ok((view, token))
}

pub async fn archive_scoped_api_key(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    key_id: &str,
) -> Result<ScopedApiKeyView, AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    let transaction = db.begin().await?;
    let current = ApiKeyOwnerRow::find_by_statement(statement(
        "SELECT key_type,user_id,lifecycle FROM api_keys WHERE id=? AND project_id=?",
        vec![key_id.into(), project_id.into()],
    ))
    .one(&transaction)
    .await?
    .ok_or(AccessError::NotFound)?;
    if current.lifecycle == "archived" {
        return Err(AccessError::Conflict("API key is already archived".into()));
    }
    let archived_at = db::now();
    transaction
        .execute(statement(
            "UPDATE api_keys SET lifecycle='archived',archived_at=?,enabled=0 WHERE id=? AND project_id=? AND lifecycle='active'",
            vec![archived_at.into(), key_id.into(), project_id.into()],
        ))
        .await?;
    audit(
        &transaction,
        actor,
        "archive",
        "api_key",
        key_id,
        json!({"project_id":project_id,"archived_at":archived_at}),
    )
    .await?;
    transaction.commit().await?;
    ScopedApiKeyView::find_by_statement(statement(
        "SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,last_used_at,allowed_ips_json,denied_ips_json,lifecycle,archived_at FROM api_keys WHERE id=? AND project_id=?",
        vec![key_id.into(), project_id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)
}

pub async fn archive_scoped_api_keys(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    key_ids: &[String],
) -> Result<BulkApiKeyArchiveResult, AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    let transaction = db.begin().await?;
    let mut archived_ids = Vec::new();
    for key_id in key_ids.iter().collect::<BTreeSet<_>>() {
        let Some(current) = ApiKeyOwnerRow::find_by_statement(statement(
            "SELECT key_type,user_id,lifecycle FROM api_keys WHERE id=? AND project_id=?",
            vec![key_id.as_str().into(), project_id.into()],
        ))
        .one(&transaction)
        .await?
        else {
            continue;
        };
        if current.lifecycle == "archived" {
            continue;
        }
        archived_ids.push((*key_id).clone());
    }
    let archived_at = db::now();
    for key_id in &archived_ids {
        transaction
            .execute(statement(
                "UPDATE api_keys SET lifecycle='archived',archived_at=?,enabled=0 WHERE id=? AND project_id=? AND lifecycle='active'",
                vec![archived_at.into(), key_id.clone().into(), project_id.into()],
            ))
            .await?;
    }
    let archived_count = archived_ids.len();
    audit(
        &transaction,
        actor,
        "archive_bulk",
        "api_key",
        project_id,
        json!({"project_id":project_id,"archived_ids":archived_ids,"archived_count":archived_count,"archived_at":archived_at}),
    )
    .await?;
    transaction.commit().await?;
    Ok(BulkApiKeyArchiveResult {
        archived_ids,
        archived_count,
    })
}

/// Bulk enable/disable for the keys one project owns. This is the same lifecycle as
/// [`update_scoped_api_key`] — the same effective `api_key:manage` authorization and
/// the same owner-membership rule for `user`/`personal` keys — because the generic
/// table toggle in the operations API could express neither.
///
/// One transaction covers the whole batch: a key that cannot be enabled leaves every
/// key in the batch exactly as it was. An id this project does not own is skipped, as
/// it is for every other bulk resource, so the batch cannot write to another
/// project's keys; the count returned is the number of keys this project changed, and
/// a caller allowed to run this can already list this project's keys, so the count
/// discloses nothing about another project. Every applied change is audited in the
/// same transaction.
pub async fn set_scoped_api_keys_enabled(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    key_ids: &[String],
    enabled: bool,
) -> Result<usize, AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    let transaction = db.begin().await?;
    let mut updated = 0usize;
    for key_id in key_ids {
        let Some(current) = ApiKeyOwnerRow::find_by_statement(statement(
            "SELECT key_type,user_id,lifecycle FROM api_keys WHERE id=? AND project_id=?",
            vec![key_id.clone().into(), project_id.into()],
        ))
        .one(&transaction)
        .await?
        else {
            continue;
        };
        if current.lifecycle == "archived" {
            return Err(AccessError::Conflict(
                "archived API keys cannot be modified".into(),
            ));
        }
        if enabled {
            ensure_owner_can_hold_enabled_key(
                &transaction,
                project_id,
                &current.key_type,
                current.user_id.as_deref(),
            )
            .await?;
        }
        transaction
            .execute(statement(
                "UPDATE api_keys SET enabled=? WHERE id=? AND project_id=?",
                vec![enabled.into(), key_id.clone().into(), project_id.into()],
            ))
            .await?;
        audit(
            &transaction,
            actor,
            "update",
            "api_key",
            key_id,
            json!({"project_id":project_id,"enabled":enabled,"bulk":true}),
        )
        .await?;
        updated += 1;
    }
    transaction.commit().await?;
    Ok(updated)
}

pub async fn delete_scoped_api_key(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    key_id: &str,
) -> Result<(), AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    let transaction = db.begin().await?;
    audit(
        &transaction,
        actor,
        "delete",
        "api_key",
        key_id,
        json!({"project_id":project_id}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM api_keys WHERE id=? AND project_id=?",
            vec![key_id.into(), project_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn list_role_bindings(
    db: &DatabaseConnection,
    actor: &Principal,
    user_id: &str,
) -> Result<Vec<RoleBindingView>, AccessError> {
    authorize(db, actor, None, "user:manage").await?;
    Ok(RoleBindingView::find_by_statement(statement("SELECT id,user_id,role_id,project_id,created_at FROM user_role_bindings WHERE user_id=? ORDER BY project_id,created_at,id", vec![user_id.into()])).all(db).await?)
}

/// Bindings a user holds **within one project**. Creating a binding needs
/// `role:manage` on that project, so reading them needs the same permission:
/// listing only through the instance-level route left a project manager able to
/// grant a role but unable to see or revoke it.
pub async fn list_project_role_bindings(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    user_id: &str,
) -> Result<Vec<RoleBindingView>, AccessError> {
    authorize(db, actor, Some(project_id), "role:manage").await?;
    Ok(RoleBindingView::find_by_statement(statement(
        "SELECT id,user_id,role_id,project_id,created_at FROM user_role_bindings WHERE user_id=? AND project_id=? ORDER BY created_at",
        vec![user_id.into(), project_id.into()],
    ))
    .all(db)
    .await?)
}

pub async fn create_role_binding(
    db: &DatabaseConnection,
    actor: &Principal,
    user_id: &str,
    input: &RoleBindingInput,
) -> Result<RoleBindingView, AccessError> {
    if let Some(project_id) = input.project_id.as_deref() {
        authorize(db, actor, Some(project_id), "role:manage").await?;
    } else {
        authorize(db, actor, None, "user:manage").await?;
    }
    ensure_can_grant_role(db, actor, input.project_id.as_deref(), &input.role_id).await?;
    let id = Uuid::new_v4().to_string();
    let transaction = db.begin().await?;
    transaction.execute(statement("INSERT INTO user_role_bindings(id,user_id,role_id,project_id,created_at) VALUES(?,?,?,?,?)", vec![id.clone().into(), user_id.into(), input.role_id.clone().into(), input.project_id.clone().into(), db::now().into()])).await.map_err(|error| AccessError::Invalid(error.to_string()))?;
    audit(
        &transaction,
        actor,
        "create",
        "user_role_binding",
        &id,
        json!({"user_id":user_id,"role_id":input.role_id,"project_id":input.project_id}),
    )
    .await?;
    transaction.commit().await?;
    RoleBindingView::find_by_statement(statement(
        "SELECT id,user_id,role_id,project_id,created_at FROM user_role_bindings WHERE id=?",
        vec![id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)
}

pub async fn delete_role_binding(
    db: &DatabaseConnection,
    actor: &Principal,
    user_id: &str,
    binding_id: &str,
) -> Result<(), AccessError> {
    #[derive(FromQueryResult)]
    struct BindingProject {
        project_id: Option<String>,
    }
    let binding = BindingProject::find_by_statement(statement(
        "SELECT project_id FROM user_role_bindings WHERE id=? AND user_id=?",
        vec![binding_id.into(), user_id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)?;
    if let Some(project_id) = binding.project_id.as_deref() {
        authorize(db, actor, Some(project_id), "role:manage").await?;
    } else {
        authorize(db, actor, None, "user:manage").await?;
    }
    if actor.user_id.as_deref() == Some(user_id) && binding.project_id.is_none() {
        return Err(AccessError::Invalid(
            "cannot remove the current user's global role binding".into(),
        ));
    }
    let transaction = db.begin().await?;
    audit(
        &transaction,
        actor,
        "delete",
        "user_role_binding",
        binding_id,
        json!({"user_id":user_id,"project_id":binding.project_id}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM user_role_bindings WHERE id=? AND user_id=?",
            vec![binding_id.into(), user_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SetupRequest;

    async fn owner_database() -> (DatabaseConnection, Principal) {
        let database = db::connect("sqlite::memory:").await.unwrap();
        let owner = db::create_initial_admin(
            &database,
            &SetupRequest {
                email: "owner@example.com".into(),
                password: "a secure password".into(),
                instance_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        (database, Principal::session(owner.id))
    }

    #[tokio::test]
    async fn permission_catalog_and_role_writes_share_delegation_boundaries() {
        let (database, owner) = owner_database().await;
        let owner_catalog = permission_catalog(&database, &owner, db::DEFAULT_PROJECT_ID)
            .await
            .unwrap();
        assert_eq!(owner_catalog.len(), 5);
        assert!(
            owner_catalog
                .iter()
                .any(|permission| permission.slug == "gateway:use"),
            "a wildcard system session receives the full applicable project catalog"
        );
        let manager = create_user(
            &database,
            &owner,
            &UserInput {
                email: "permission-manager@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
        let manager_role = create_role(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &RoleInput {
                name: "permission manager".into(),
                permissions: vec!["project:manage".into()],
            },
        )
        .await
        .unwrap();
        upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: manager.id.clone(),
                role_id: manager_role.id,
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let actor = Principal::session(manager.id);

        let catalog = permission_catalog(&database, &actor, db::DEFAULT_PROJECT_ID)
            .await
            .unwrap();
        let slugs = catalog
            .iter()
            .map(|permission| permission.slug.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            slugs,
            BTreeSet::from([
                "api_key:manage",
                "project:manage",
                "project:read",
                "role:manage",
            ]),
            "project:manage must keep its documented implications without granting gateway use"
        );
        assert!(
            catalog
                .iter()
                .all(|permission| permission.level == "project")
        );
        assert!(
            catalog
                .iter()
                .all(|permission| !permission.description.trim().is_empty())
        );

        let delegated = create_role(
            &database,
            &actor,
            db::DEFAULT_PROJECT_ID,
            &RoleInput {
                name: "delegated manager".into(),
                permissions: vec!["project:read".into(), "api_key:manage".into()],
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            create_role(
                &database,
                &actor,
                db::DEFAULT_PROJECT_ID,
                &RoleInput {
                    name: "escalated creator".into(),
                    permissions: vec!["gateway:use".into()],
                },
            )
            .await,
            Err(AccessError::Forbidden)
        ));
        assert!(matches!(
            update_role(
                &database,
                &actor,
                db::DEFAULT_PROJECT_ID,
                &delegated.id,
                &RoleInput {
                    name: "escalated editor".into(),
                    permissions: vec!["project:read".into(), "gateway:use".into()],
                },
            )
            .await,
            Err(AccessError::Forbidden)
        ));
        let unchanged = list_roles(&database, &actor, db::DEFAULT_PROJECT_ID)
            .await
            .unwrap()
            .into_iter()
            .find(|role| role.id == delegated.id)
            .unwrap();
        assert_eq!(unchanged.name, "delegated manager");
        assert_eq!(
            unchanged.permissions,
            "[\"project:read\",\"api_key:manage\"]"
        );
    }

    /// A roster of opaque user ids cannot be read by the project manager who is
    /// allowed to see it, and that manager is refused the user list, so the
    /// identity has to travel with the membership.
    #[tokio::test]
    async fn the_member_roster_carries_the_member_identity() {
        let (database, owner) = owner_database().await;
        let project = create_project(
            &database,
            &owner,
            &ProjectInput {
                name: "Roster".into(),
                slug: "roster".into(),
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        let user = create_user(
            &database,
            &owner,
            &UserInput {
                email: "roster-member@example.com".into(),
                password: "another secure password".into(),
                display_name: Some("Roster Member".into()),
                language: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
        let reader = create_role(
            &database,
            &owner,
            &project.id,
            &RoleInput {
                name: "reader".into(),
                permissions: vec!["project:read".into()],
            },
        )
        .await
        .unwrap();
        let created = upsert_membership(
            &database,
            &owner,
            &project.id,
            &MembershipInput {
                user_id: user.id.clone(),
                role_id: reader.id.clone(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            created.email.as_deref(),
            Some("roster-member@example.com"),
            "the add response must identify the member too"
        );

        let roster = list_memberships(&database, &owner, &project.id)
            .await
            .unwrap();
        let member = roster
            .iter()
            .find(|row| row.user_id == user.id)
            .expect("the member must be in the roster");
        assert_eq!(
            member.email.as_deref(),
            Some("roster-member@example.com"),
            "the roster must say who the member is"
        );
        assert_eq!(member.display_name.as_deref(), Some("Roster Member"));
    }

    /// The console offered an `enabled` switch on the create-user form and the
    /// backend ignored it, always inserting an enabled account.
    #[tokio::test]
    async fn creating_a_user_honours_the_enabled_switch() {
        let (database, owner) = owner_database().await;
        let created = create_user(
            &database,
            &owner,
            &UserInput {
                email: "disabled@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
                enabled: false,
            },
        )
        .await
        .unwrap();
        let row = UserView::find_by_statement(statement(
            "SELECT id,email,display_name,role,language,theme,avatar_url,enabled,created_at,updated_at FROM users WHERE id=?",
            vec![created.id.clone().into()],
        ))
        .one(&database)
        .await
        .unwrap()
        .expect("the user must exist");
        assert!(
            !row.enabled,
            "a user created with the switch off must not be enabled"
        );
    }

    /// A key that has run out of budget or expired must be distinguishable from a
    /// working one. The scoped view the console reads carried the budget but not
    /// the spend, so a key that could no longer be used still read as healthy.
    #[tokio::test]
    async fn the_key_view_carries_expiry_and_spend() {
        let (database, owner) = owner_database().await;
        database
            .execute(statement(
                "INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,scopes,budget_micros,spent_micros,enabled,created_at,project_id,expires_at) VALUES('key-spent','spent','spent-prefix','hash','digest','[\"gateway\"]',1000,1000,1,0,?,?)",
                vec![
                    db::DEFAULT_PROJECT_ID.into(),
                    (db::now() - 60).into(),
                ],
            ))
            .await
            .unwrap();

        let rows = list_scoped_api_keys(&database, &owner, db::DEFAULT_PROJECT_ID)
            .await
            .unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "key-spent")
            .expect("the key must be listed");
        assert!(
            row.expires_at.is_some_and(|at| at < db::now()),
            "an expired key must be recognisable as expired"
        );
        assert_eq!(
            row.spent_micros, 1000,
            "the spend must travel with the budget, or a key out of budget reads as healthy"
        );
        assert!(
            row.last_used_at.is_none(),
            "an unused key must be distinguishable from one in active use"
        );
    }

    #[tokio::test]
    async fn the_key_view_serializes_scopes_as_an_array() {
        let (database, owner) = owner_database().await;
        let (key, _) = create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "scope shape".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                key_type: "service".into(),
                scopes: vec!["gateway:use".into(), "project:read".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            serde_json::to_value(key).unwrap()["scopes"],
            json!(["gateway:use", "project:read"]),
            "the HTTP view must expose project scopes as an array, not a JSON string"
        );
    }

    #[tokio::test]
    async fn an_api_key_cannot_delegate_a_scope_it_does_not_hold() {
        let (database, _owner) = owner_database().await;
        let actor = Principal::api_key(
            "parent-key",
            db::DEFAULT_PROJECT_ID,
            None,
            vec!["api_key:manage".into()],
        );

        assert!(matches!(
            create_scoped_api_key(
                &database,
                &actor,
                &ScopedApiKeyInput {
                    name: "escalated child".into(),
                    project_id: db::DEFAULT_PROJECT_ID.into(),
                    key_type: "service".into(),
                    scopes: vec!["api_key:manage".into(), "gateway:use".into()],
                    ..Default::default()
                },
            )
            .await,
            Err(AccessError::Forbidden)
        ));
        assert!(
            list_scoped_api_keys(&database, &actor, db::DEFAULT_PROJECT_ID)
                .await
                .unwrap()
                .is_empty(),
            "a refused delegation must not leave a child key behind"
        );
    }

    #[tokio::test]
    async fn denies_cross_project_access_and_resolves_project_roles() {
        let (database, owner) = owner_database().await;
        let user = create_user(
            &database,
            &owner,
            &UserInput {
                email: "member@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
        let project_a = create_project(
            &database,
            &owner,
            &ProjectInput {
                name: "A".into(),
                slug: "a".into(),
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        let project_b = create_project(
            &database,
            &owner,
            &ProjectInput {
                name: "B".into(),
                slug: "b".into(),
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        let reader = create_role(
            &database,
            &owner,
            &project_a.id,
            &RoleInput {
                name: "reader".into(),
                permissions: vec!["project:read".into()],
            },
        )
        .await
        .unwrap();
        upsert_membership(
            &database,
            &owner,
            &project_a.id,
            &MembershipInput {
                user_id: user.id.clone(),
                role_id: reader.id.clone(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let member = Principal::session(user.id);
        assert!(get_project(&database, &member, &project_a.id).await.is_ok());
        assert!(matches!(
            get_project(&database, &member, &project_b.id).await,
            Err(AccessError::Forbidden)
        ));
        assert!(matches!(
            authorize(&database, &member, Some(&project_a.id), "project:manage").await,
            Err(AccessError::Forbidden)
        ));
        let unjoined = create_user(
            &database,
            &owner,
            &UserInput {
                email: "unjoined@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
        create_role_binding(
            &database,
            &owner,
            &unjoined.id,
            &RoleBindingInput {
                role_id: reader.id,
                project_id: Some(project_a.id.clone()),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            get_project(&database, &Principal::session(unjoined.id), &project_a.id).await,
            Err(AccessError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn invitation_is_hashed_expiring_and_one_time() {
        let (database, owner) = owner_database().await;
        let (invitation, token) = create_invitation(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &InvitationInput {
                email: "invitee@example.com".into(),
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                expires_in_seconds: Some(60),
            },
        )
        .await
        .unwrap();
        assert_ne!(invitation.id, token);
        let accepted = accept_invitation(
            &database,
            None,
            &AcceptInvitationInput {
                token: token.clone(),
                password: Some("an invited password".into()),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(accepted.email, "invitee@example.com");
        assert!(matches!(
            inspect_invitation(&database, &token).await,
            Err(AccessError::NotFound)
        ));
        assert!(matches!(
            accept_invitation(
                &database,
                Some(&Principal::session(accepted.id.clone())),
                &AcceptInvitationInput {
                    token,
                    password: None,
                    display_name: None,
                    language: None
                }
            )
            .await,
            Err(AccessError::NotFound)
        ));
    }

    #[tokio::test]
    async fn api_key_principals_are_confined_to_their_project_and_scopes() {
        let (database, owner) = owner_database().await;
        let project_a = create_project(
            &database,
            &owner,
            &ProjectInput {
                name: "A".into(),
                slug: "key-a".into(),
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        let project_b = create_project(
            &database,
            &owner,
            &ProjectInput {
                name: "B".into(),
                slug: "key-b".into(),
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        let (key, _) = create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "reader".into(),
                project_id: project_a.id.clone(),
                user_id: None,
                profile_id: None,
                key_type: "service".into(),
                scopes: vec!["project:read".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let principal = Principal::api_key(
            key.id,
            project_a.id.clone(),
            None,
            vec!["project:read".into()],
        );
        assert!(
            get_project(&database, &principal, &project_a.id)
                .await
                .is_ok()
        );
        assert!(matches!(
            get_project(&database, &principal, &project_b.id).await,
            Err(AccessError::Forbidden)
        ));
        assert!(matches!(
            authorize(&database, &principal, None, "user:manage").await,
            Err(AccessError::Forbidden)
        ));
    }

    /// Creates a member of the default project and an owned key for them, so the
    /// bulk lifecycle tests can move that owner's membership around.
    async fn owned_key(
        database: &DatabaseConnection,
        owner: &Principal,
        key_type: &str,
        status: &str,
    ) -> (String, String) {
        let member = create_user(
            database,
            owner,
            &UserInput {
                email: format!("{key_type}-owner@example.com"),
                password: "another secure password".into(),
                display_name: None,
                language: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
        upsert_membership(
            database,
            owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: member.id.clone(),
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                status: status.into(),
            },
        )
        .await
        .unwrap();
        let (key, _) = create_scoped_api_key(
            database,
            owner,
            &ScopedApiKeyInput {
                name: format!("{key_type} key"),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: Some(member.id.clone()),
                profile_id: None,
                key_type: key_type.into(),
                scopes: vec!["gateway:use".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        (member.id, key.id)
    }

    /// Enables one key through the bulk lifecycle, which is the path under test.
    async fn enable_key(
        database: &DatabaseConnection,
        owner: &Principal,
        key_id: &str,
    ) -> Result<usize, AccessError> {
        set_scoped_api_keys_enabled(
            database,
            owner,
            db::DEFAULT_PROJECT_ID,
            &[key_id.to_owned()],
            true,
        )
        .await
    }

    async fn enabled_state(database: &DatabaseConnection, key_id: &str) -> bool {
        PermissionRow::find_by_statement(statement(
            "SELECT CAST(enabled AS TEXT) AS slug FROM api_keys WHERE id=?",
            vec![key_id.into()],
        ))
        .one(database)
        .await
        .unwrap()
        .unwrap()
        .slug
            == "1"
    }

    /// Creates a key owned by nobody, for the project this caller owns.
    async fn service_key(
        database: &DatabaseConnection,
        owner: &Principal,
        project_id: &str,
        name: &str,
    ) -> String {
        create_scoped_api_key(
            database,
            owner,
            &ScopedApiKeyInput {
                name: name.into(),
                project_id: project_id.into(),
                user_id: None,
                profile_id: None,
                key_type: "service".into(),
                scopes: vec!["gateway:use".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .0
        .id
    }

    /// A member of the default project holding only the system member role.
    async fn member(database: &DatabaseConnection, owner: &Principal, email: &str) -> Principal {
        let user = create_user(
            database,
            owner,
            &UserInput {
                email: email.into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
        upsert_membership(
            database,
            owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: user.id.clone(),
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        Principal::session(user.id)
    }

    /// The bulk lifecycle and the single-key update are one rule. A `user`/`personal`
    /// key acts as its owner, so enabling it while that owner is suspended, disabled,
    /// gone or no longer a member must be refused — and refused with the same stated
    /// reason, because a second copy of that query is exactly what drifts.
    #[tokio::test]
    async fn bulk_key_state_applies_the_single_key_owner_rule() {
        let (database, owner) = owner_database().await;
        let (owner_id, key) = owned_key(&database, &owner, "personal", "active").await;
        assert!(
            set_scoped_api_keys_enabled(
                &database,
                &owner,
                db::DEFAULT_PROJECT_ID,
                std::slice::from_ref(&key),
                false
            )
            .await
            .is_ok()
        );
        assert!(!enabled_state(&database, &key).await);

        // Suspended owner: the bulk path must state the same rule as the single-key
        // update, word for word.
        upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: owner_id.clone(),
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                status: "suspended".into(),
            },
        )
        .await
        .unwrap();
        let bulk = enable_key(&database, &owner, &key).await;
        let one = update_scoped_api_key(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &key,
            &ScopedApiKeyUpdate {
                enabled: Some(true),
                ..Default::default()
            },
        )
        .await;
        match (bulk, one) {
            (Err(AccessError::Invalid(bulk)), Err(AccessError::Invalid(one))) => {
                assert_eq!(bulk, one, "both paths must state the same rule")
            }
            (bulk, one) => panic!("both paths must refuse a suspended owner: {bulk:?} {one:?}"),
        }
        assert!(
            !enabled_state(&database, &key).await,
            "a refused bulk enable must change nothing"
        );

        // A disabled account, even with an active membership.
        database
            .execute(statement(
                "UPDATE users SET enabled=0 WHERE id=?",
                vec![owner_id.clone().into()],
            ))
            .await
            .unwrap();
        upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: owner_id.clone(),
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            enable_key(&database, &owner, &key).await,
            Err(AccessError::Invalid(_))
        ));

        // No membership at all.
        database
            .execute(statement(
                "DELETE FROM project_memberships WHERE user_id=?",
                vec![owner_id.clone().into()],
            ))
            .await
            .unwrap();
        database
            .execute(statement(
                "UPDATE users SET enabled=1 WHERE id=?",
                vec![owner_id.clone().into()],
            ))
            .await
            .unwrap();
        assert!(matches!(
            enable_key(&database, &owner, &key).await,
            Err(AccessError::Invalid(_))
        ));

        // "Owner missing" cannot be reached through the record system: the key's
        // `user_id` is `ON DELETE CASCADE` and the table rejects a `user`/`personal`
        // row with no owner, so the ownerless branch of the rule is defence in depth
        // rather than a state the database can hold. Deleting the owner removes the
        // key, which is why the rule is stated as a refusal instead.
        assert_eq!(
            PermissionRow::find_by_statement(statement(
                "SELECT CAST(COUNT(*) AS TEXT) AS slug FROM api_keys WHERE id=?",
                vec![key.clone().into()],
            ))
            .one(&database)
            .await
            .unwrap()
            .unwrap()
            .slug,
            "1",
            "the owned key must still exist for the remaining assertions"
        );

        // The owner is active again, and the key is still disabled: the lifecycle is
        // restored, not bypassed.
        upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: owner_id.clone(),
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        assert!(
            !enabled_state(&database, &key).await,
            "no failed enable may leave the key on"
        );

        // An active owner restores the lifecycle.
        assert_eq!(enable_key(&database, &owner, &key).await.unwrap(), 1);
        assert!(enabled_state(&database, &key).await);
    }

    /// Bulk key state is a key-lifecycle mutation, so it carries that lifecycle's
    /// authorization, stays inside the project, and fails closed on an id the project
    /// does not own — with no partial application.
    #[tokio::test]
    async fn bulk_key_state_requires_key_authority_and_stays_in_the_project() {
        let (database, owner) = owner_database().await;
        let reader = member(&database, &owner, "reader@example.com").await;
        let service = service_key(&database, &owner, db::DEFAULT_PROJECT_ID, "service").await;
        let project = create_project(
            &database,
            &owner,
            &ProjectInput {
                name: "Elsewhere".into(),
                slug: "elsewhere".into(),
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        let foreign = service_key(&database, &owner, &project.id, "foreign").await;

        // The system member role holds `project:read`, which is not the key contract.
        assert!(matches!(
            set_scoped_api_keys_enabled(
                &database,
                &reader,
                db::DEFAULT_PROJECT_ID,
                std::slice::from_ref(&service),
                false
            )
            .await,
            Err(AccessError::Forbidden)
        ));
        assert!(
            enabled_state(&database, &service).await,
            "a refused bulk change must leave the key alone"
        );

        // Another project's key is skipped exactly like an id that does not exist, so
        // the batch stays inside this project and neither id is written.
        for id in [foreign.clone(), "no-such-key".to_string()] {
            assert_eq!(
                set_scoped_api_keys_enabled(
                    &database,
                    &owner,
                    db::DEFAULT_PROJECT_ID,
                    &[id],
                    false
                )
                .await
                .unwrap(),
                0,
                "an id this project does not own is not a change"
            );
        }
        assert!(
            enabled_state(&database, &foreign).await,
            "another project's key must not be toggled"
        );

        // One unowned id in the batch does not make the owned ones change partially:
        // the batch applies what this project owns and leaves the rest alone.
        assert_eq!(
            set_scoped_api_keys_enabled(
                &database,
                &owner,
                db::DEFAULT_PROJECT_ID,
                &[service.clone(), foreign.clone()],
                false
            )
            .await
            .unwrap(),
            1
        );
        assert!(
            !enabled_state(&database, &service).await,
            "the key this project owns is the one that changed"
        );
        assert!(
            enabled_state(&database, &foreign).await,
            "the foreign key is untouched"
        );

        // An owned key that cannot be enabled rolls the whole batch back, including
        // the keys that would have succeeded on their own.
        let (member_id, owned) = owned_key(&database, &owner, "personal", "active").await;
        upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: member_id.clone(),
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                status: "suspended".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            set_scoped_api_keys_enabled(
                &database,
                &owner,
                db::DEFAULT_PROJECT_ID,
                &[service.clone(), owned.clone()],
                true
            )
            .await,
            Err(AccessError::Invalid(_))
        ));
        assert!(
            !enabled_state(&database, &service).await,
            "a refused batch must not be applied partially"
        );
        assert!(!enabled_state(&database, &owned).await);

        // With an active owner the same batch applies, and each key it touched is
        // audited.
        upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: member_id,
                role_id: SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            set_scoped_api_keys_enabled(
                &database,
                &owner,
                db::DEFAULT_PROJECT_ID,
                &[service.clone(), owned.clone()],
                true
            )
            .await
            .unwrap(),
            2
        );
        assert!(enabled_state(&database, &service).await);
        assert!(enabled_state(&database, &owned).await);
        assert_eq!(
            PermissionRow::find_by_statement(statement(
                "SELECT CAST(COUNT(*) AS TEXT) AS slug FROM audit_events WHERE resource_type='api_key' AND resource_id IN (?,?) AND action='update'",
                vec![service.clone().into(), owned.clone().into()],
            ))
            .one(&database)
            .await
            .unwrap()
            .unwrap()
            .slug,
            "3",
            "the committed rows are the service key's initial disable and the final batch's two enables; the refused batch's row rolled back with it"
        );
    }

    #[tokio::test]
    async fn rotating_a_key_replaces_the_secret_once_and_audits_the_change() {
        let (database, owner) = owner_database().await;
        let (key, old_token) = create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "rotated".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                key_type: "service".into(),
                scopes: vec!["gateway:use".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let (view, new_token) =
            rotate_scoped_api_key(&database, &owner, db::DEFAULT_PROJECT_ID, &key.id)
                .await
                .unwrap();
        assert_ne!(old_token, new_token);
        assert!(
            db::authenticate_api_key(&database, &old_token, None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db::authenticate_api_key(&database, &new_token, None)
                .await
                .unwrap()
                .is_some()
        );
        assert!(!serde_json::to_string(&view).unwrap().contains(&new_token));
        assert_eq!(
            PermissionRow::find_by_statement(statement(
                "SELECT CAST(COUNT(*) AS TEXT) AS slug FROM audit_events WHERE resource_type='api_key' AND resource_id=? AND action='rotate'",
                vec![key.id.into()],
            ))
            .one(&database)
            .await
            .unwrap()
            .unwrap()
            .slug,
            "1"
        );
    }

    #[tokio::test]
    async fn rotation_rolls_back_when_the_audit_write_fails() {
        let (database, owner) = owner_database().await;
        let (key, old_token) = create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "rollback".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                key_type: "service".into(),
                scopes: vec!["gateway:use".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        database
            .execute_unprepared("CREATE TRIGGER fail_rotate_audit BEFORE INSERT ON audit_events WHEN NEW.action='rotate' BEGIN SELECT RAISE(ABORT,'audit refused'); END;")
            .await
            .unwrap();

        assert!(
            rotate_scoped_api_key(&database, &owner, db::DEFAULT_PROJECT_ID, &key.id)
                .await
                .is_err()
        );
        assert!(
            db::authenticate_api_key(&database, &old_token, None)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn archived_keys_refuse_authentication_and_every_mutable_path() {
        let (database, owner) = owner_database().await;
        let (key, token) = create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "archive me".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                key_type: "service".into(),
                scopes: vec!["gateway:use".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let archived = archive_scoped_api_key(&database, &owner, db::DEFAULT_PROJECT_ID, &key.id)
            .await
            .unwrap();
        assert_eq!(archived.lifecycle, "archived");
        assert!(archived.archived_at.is_some());
        assert_eq!(
            PermissionRow::find_by_statement(statement(
                "SELECT CAST(COUNT(*) AS TEXT) AS slug FROM audit_events WHERE action='archive' AND resource_type='api_key' AND resource_id=?",
                vec![key.id.clone().into()],
            )).one(&database).await.unwrap().unwrap().slug,
            "1"
        );
        assert!(
            db::authenticate_api_key(&database, &token, None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            update_scoped_api_key(
                &database,
                &owner,
                db::DEFAULT_PROJECT_ID,
                &key.id,
                &ScopedApiKeyUpdate {
                    enabled: Some(true),
                    ..Default::default()
                }
            )
            .await,
            Err(AccessError::Conflict(_))
        ));
        assert!(matches!(
            rotate_scoped_api_key(&database, &owner, db::DEFAULT_PROJECT_ID, &key.id).await,
            Err(AccessError::Conflict(_))
        ));
        assert!(matches!(
            set_scoped_api_keys_enabled(&database, &owner, db::DEFAULT_PROJECT_ID, &[key.id], true)
                .await,
            Err(AccessError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn bulk_archive_deduplicates_skips_foreign_ids_and_records_actual_facts() {
        let (database, owner) = owner_database().await;
        let local = service_key(&database, &owner, db::DEFAULT_PROJECT_ID, "local").await;
        let project = create_project(
            &database,
            &owner,
            &ProjectInput {
                name: "Foreign".into(),
                slug: "foreign-archive".into(),
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        let foreign = service_key(&database, &owner, &project.id, "foreign").await;

        let result = archive_scoped_api_keys(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &[
                local.clone(),
                local.clone(),
                foreign.clone(),
                "missing".into(),
            ],
        )
        .await
        .unwrap();
        assert_eq!(result.archived_ids, vec![local.clone()]);
        assert_eq!(result.archived_count, 1);
        let audit = PermissionRow::find_by_statement(statement(
            "SELECT details AS slug FROM audit_events WHERE action='archive_bulk' AND resource_type='api_key' ORDER BY created_at DESC LIMIT 1",
            vec![],
        )).one(&database).await.unwrap().unwrap().slug;
        let audit: Value = serde_json::from_str(&audit).unwrap();
        assert_eq!(audit["archived_ids"], json!([local]));
        assert_eq!(audit["archived_count"], 1);
        assert_eq!(
            PermissionRow::find_by_statement(statement(
                "SELECT lifecycle AS slug FROM api_keys WHERE id=?",
                vec![foreign.into()],
            ))
            .one(&database)
            .await
            .unwrap()
            .unwrap()
            .slug,
            "active"
        );
    }

    #[tokio::test]
    async fn archive_can_revoke_suspended_owner_keys_while_rotate_refuses_them() {
        let (database, owner) = owner_database().await;
        let (owner_id, first) = owned_key(&database, &owner, "personal", "active").await;
        let (second_owner_id, second) = owned_key(&database, &owner, "user", "active").await;
        database.execute(statement(
            "UPDATE project_memberships SET status='suspended' WHERE project_id=? AND user_id IN (?,?)",
            vec![db::DEFAULT_PROJECT_ID.into(), owner_id.into(), second_owner_id.into()],
        )).await.unwrap();

        assert!(matches!(
            rotate_scoped_api_key(&database, &owner, db::DEFAULT_PROJECT_ID, &first).await,
            Err(AccessError::Invalid(_))
        ));
        assert!(
            archive_scoped_api_key(&database, &owner, db::DEFAULT_PROJECT_ID, &first)
                .await
                .is_ok()
        );
        let bulk = archive_scoped_api_keys(&database, &owner, db::DEFAULT_PROJECT_ID, &[second])
            .await
            .unwrap();
        assert_eq!(bulk.archived_count, 1);
    }

    #[tokio::test]
    async fn single_and_bulk_archive_require_api_key_manage() {
        let (database, owner) = owner_database().await;
        let key = service_key(&database, &owner, db::DEFAULT_PROJECT_ID, "protected").await;
        let member = member(&database, &owner, "archive-member@example.com").await;
        assert!(matches!(
            archive_scoped_api_key(&database, &member, db::DEFAULT_PROJECT_ID, &key).await,
            Err(AccessError::Forbidden)
        ));
        assert!(matches!(
            archive_scoped_api_keys(&database, &member, db::DEFAULT_PROJECT_ID, &[key]).await,
            Err(AccessError::Forbidden)
        ));
    }
}
