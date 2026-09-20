use std::collections::BTreeSet;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
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
    let project_manage = project_id.is_some()
        && matches!(
            permission,
            "project:read" | "role:manage" | "api_key:manage"
        )
        && scopes.contains("project:manage");
    if scopes.contains("*") || scopes.contains(permission) || project_manage {
        Ok(())
    } else {
        Err(AccessError::Forbidden)
    }
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

#[derive(Debug, Deserialize)]
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
}

#[derive(Debug, Deserialize)]
pub struct ScopedApiKeyUpdate {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub expires_at: Option<i64>,
    pub scopes: Option<Vec<String>>,
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
    pub scopes: String,
    pub budget_micros: Option<i64>,
    pub enabled: bool,
    pub created_at: i64,
    pub project_id: String,
    pub user_id: Option<String>,
    pub profile_id: Option<String>,
    pub key_type: String,
    pub expires_at: Option<i64>,
    pub allowed_ips_json: String,
    pub denied_ips_json: String,
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
        "INSERT INTO users(id,email,password_hash,role,language,theme,created_at,display_name,enabled,updated_at) VALUES(?,?,?,'member',?,'system:bronze',?,?,1,?)",
        vec![id.clone().into(), email.clone().into(), password_hash.into(), input.language.clone().unwrap_or_else(|| "zh-CN".into()).into(), timestamp.into(), display_name.into(), timestamp.into()],
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
        "SELECT id,project_id,name,scope,is_system,created_at,updated_at FROM roles WHERE project_id IS NULL OR project_id=? ORDER BY is_system DESC,name",
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
        "SELECT id,project_id,name,scope,is_system,created_at,updated_at FROM roles WHERE id=?",
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
        "SELECT id,project_id,name,scope,is_system,created_at,updated_at FROM roles WHERE id=?",
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
    Ok(MembershipView::find_by_statement(statement("SELECT id,project_id,user_id,role_id,status,created_at,updated_at FROM project_memberships WHERE project_id=? ORDER BY created_at,id", vec![project_id.into()])).all(db).await?)
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
    MembershipView::find_by_statement(statement("SELECT id,project_id,user_id,role_id,status,created_at,updated_at FROM project_memberships WHERE project_id=? AND user_id=?", vec![project_id.into(), input.user_id.clone().into()])).one(db).await?.ok_or(AccessError::NotFound)
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
    {
        #[derive(FromQueryResult)]
        struct MembershipExists {
            present: i64,
        }
        let membership = MembershipExists::find_by_statement(statement("SELECT 1 AS present FROM project_memberships membership JOIN users user ON user.id=membership.user_id AND user.enabled=1 WHERE membership.project_id=? AND membership.user_id=? AND membership.status='active'", vec![input.project_id.clone().into(), user_id.clone().into()])).one(db).await?;
        if !membership.is_some_and(|row| row.present == 1) {
            return Err(AccessError::Invalid(
                "user/personal API-key owner must be an active user and project member".into(),
            ));
        }
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
    let prefix = crypto::opaque_token("").chars().take(8).collect::<String>();
    let token = format!("pg_{prefix}_{}", crypto::opaque_token(""));
    let id = Uuid::new_v4().to_string();
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO api_keys(id,name,key_prefix,key_hash,scopes,budget_micros,spent_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,allowed_ips_json,denied_ips_json) VALUES(?,?,?,?,?,?,0,1,?,?,?,?,?,?,?,?)",
        vec![id.clone().into(), name.clone().into(), prefix.clone().into(), crypto::hash_password(&token).map_err(AccessError::Internal)?.into(), serde_json::to_string(&scopes).map_err(|e| AccessError::Internal(e.into()))?.into(), input.budget_micros.into(), db::now().into(), input.project_id.clone().into(), input.user_id.clone().into(), input.profile_id.clone().into(), input.key_type.clone().into(), input.expires_at.into(), serde_json::to_string(&input.allowed_ips).map_err(|e| AccessError::Internal(e.into()))?.into(), serde_json::to_string(&input.denied_ips).map_err(|e| AccessError::Internal(e.into()))?.into()],
    )).await.map_err(|error| AccessError::Invalid(error.to_string()))?;
    audit(&transaction, actor, "create", "api_key", &id, json!({"project_id":input.project_id,"name":name,"prefix":prefix,"key_type":input.key_type})).await?;
    transaction.commit().await?;
    let view = ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,allowed_ips_json,denied_ips_json FROM api_keys WHERE id=?", vec![id.into()])).one(db).await?.ok_or(AccessError::NotFound)?;
    Ok((view, token))
}

pub async fn list_scoped_api_keys(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
) -> Result<Vec<ScopedApiKeyView>, AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    Ok(ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,allowed_ips_json,denied_ips_json FROM api_keys WHERE project_id=? ORDER BY created_at DESC,id", vec![project_id.into()])).all(db).await?)
}

pub async fn update_scoped_api_key(
    db: &DatabaseConnection,
    actor: &Principal,
    project_id: &str,
    key_id: &str,
    input: &ScopedApiKeyUpdate,
) -> Result<ScopedApiKeyView, AccessError> {
    authorize(db, actor, Some(project_id), "api_key:manage").await?;
    if input.expires_at.is_some_and(|expires| expires <= db::now()) {
        return Err(AccessError::Invalid(
            "expiration must be in the future".into(),
        ));
    }
    let current = ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,allowed_ips_json,denied_ips_json FROM api_keys WHERE id=? AND project_id=?", vec![key_id.into(), project_id.into()])).one(db).await?.ok_or(AccessError::NotFound)?;
    if input.enabled == Some(true) && matches!(current.key_type.as_str(), "user" | "personal") {
        let Some(owner_id) = current.user_id.as_deref() else {
            return Err(AccessError::Invalid("owned API key has no owner".into()));
        };
        let owner_active = PermissionRow::find_by_statement(statement(
            "SELECT 'active' AS slug FROM users user JOIN project_memberships membership ON membership.user_id=user.id AND membership.project_id=? AND membership.status='active' WHERE user.id=? AND user.enabled=1",
            vec![project_id.into(), owner_id.into()],
        ))
        .one(db)
        .await?
        .is_some();
        if !owner_active {
            return Err(AccessError::Invalid(
                "cannot enable a user/personal key without an active owner membership".into(),
            ));
        }
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
    let transaction = db.begin().await?;
    transaction.execute(statement("UPDATE api_keys SET name=?,enabled=?,expires_at=?,scopes=? WHERE id=? AND project_id=?", vec![name.clone().into(), input.enabled.unwrap_or(current.enabled).into(), input.expires_at.or(current.expires_at).into(), scopes.into(), key_id.into(), project_id.into()])).await?;
    audit(
        &transaction,
        actor,
        "update",
        "api_key",
        key_id,
        json!({"project_id":project_id,"name":name,"enabled":input.enabled}),
    )
    .await?;
    transaction.commit().await?;
    ScopedApiKeyView::find_by_statement(statement("SELECT id,name,key_prefix,scopes,budget_micros,enabled,created_at,project_id,user_id,profile_id,key_type,expires_at,allowed_ips_json,denied_ips_json FROM api_keys WHERE id=? AND project_id=?", vec![key_id.into(), project_id.into()])).one(db).await?.ok_or(AccessError::NotFound)
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
}
