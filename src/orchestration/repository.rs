use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde_json::{Value, json};
use std::collections::BTreeSet;

use super::{
    Candidate, Decision, Error, Profile, Result,
    policy::{self, CircuitPolicy, Limits, Retry, Routing},
};
use crate::{
    db,
    models::{ApiKeyCredential, RouteTarget},
};

pub(super) fn statement(sql: &str, args: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, args)
}

pub async fn profile(db: &DatabaseConnection, key: &ApiKeyCredential) -> Result<Profile> {
    #[derive(FromQueryResult)]
    struct Row {
        id: String,
        rpm_limit: Option<i64>,
        tpm_limit: Option<i64>,
        budget_micros: Option<i64>,
        routing_policy_json: String,
        spent: i64,
    }
    let project = db
        .query_one(statement(
            "SELECT settings_json FROM projects WHERE id=? AND enabled=1",
            vec![key.project_id.clone().into()],
        ))
        .await?
        .ok_or(Error::Forbidden)?;
    let settings = policy::document(&project.try_get::<String>("", "settings_json")?)?;
    let defaults = settings
        .get("routing")
        .cloned()
        .unwrap_or(json!({"version":1}));
    let row = Row::find_by_statement(statement("SELECT p.id,p.rpm_limit,p.tpm_limit,p.budget_micros,p.routing_policy_json,(SELECT COALESCE(SUM(spent_micros),0) FROM api_keys WHERE profile_id=p.id) AS spent FROM api_keys k JOIN api_key_profiles p ON p.id=k.profile_id AND p.project_id=k.project_id WHERE k.id=? AND k.project_id=?",vec![key.id.clone().into(),key.project_id.clone().into()])).one(db).await?;
    let Some(row) = row else {
        return Ok(Profile {
            id: None,
            budget: None,
            routing: Routing::parse(&defaults.to_string())?,
            mappings: vec![],
            allowed: vec![],
        });
    };
    if row.budget_micros.is_some_and(|budget| row.spent >= budget) {
        return Err(Error::Admission("profile_budget"));
    }
    let mut routing = Routing::parse(&row.routing_policy_json)?;
    routing.limits.rpm = row
        .rpm_limit
        .map(u32::try_from)
        .transpose()
        .map_err(|_| Error::Configuration)?;
    routing.limits.tpm = row
        .tpm_limit
        .map(u32::try_from)
        .transpose()
        .map_err(|_| Error::Configuration)?;
    let mappings = db.query_all(statement("SELECT source_model,target_model FROM api_key_profile_model_mappings WHERE profile_id=? ORDER BY priority,id",vec![row.id.clone().into()])).await?.into_iter()
        .map(|r| Ok((r.try_get("","source_model")?,r.try_get("","target_model")?))).collect::<Result<Vec<_>>>()?;
    let allowed = db.query_all(statement("SELECT model_pattern,match_type FROM api_key_profile_allowed_models WHERE profile_id=? ORDER BY model_pattern,match_type",vec![row.id.clone().into()])).await?.into_iter()
        .map(|r| Ok((r.try_get("","model_pattern")?,r.try_get("","match_type")?))).collect::<Result<Vec<_>>>()?;
    Ok(Profile {
        id: Some(row.id),
        budget: row.budget_micros,
        routing,
        mappings,
        allowed,
    })
}

#[derive(FromQueryResult)]
struct Row {
    model_id: String,
    provider_id: String,
    credential_id: String,
    public_name: String,
    upstream_name: String,
    capabilities: String,
    provider_name: String,
    provider_kind: String,
    base_url: String,
    secret_envelope: String,
    input_price_micros: i64,
    output_price_micros: i64,
    priority: i32,
    model_enabled: bool,
    provider_enabled: bool,
    settings_json: String,
    endpoint_mappings_json: String,
    model_rules_json: String,
    parameter_overrides_json: String,
    retry_statuses_json: String,
    disabled_until: Option<i64>,
    backoff_until: Option<i64>,
    quota_exhausted: bool,
}

#[derive(FromQueryResult)]
struct Association {
    id: String,
    model_id: Option<String>,
    provider_id: Option<String>,
    match_type: String,
    pattern: String,
    conditions_json: String,
    priority: i32,
    weight: i32,
}

pub async fn candidates(
    db: &DatabaseConnection,
    key: &ApiKeyCredential,
    model: &str,
    context: &Value,
    routing: &Routing,
    decisions: &mut Vec<Decision>,
) -> Result<Vec<Candidate>> {
    let rows = Row::find_by_statement(statement(r#"
        SELECT m.id AS model_id,p.id AS provider_id,c.id AS credential_id,m.public_name,m.upstream_name,m.capabilities,
        p.name AS provider_name,p.kind AS provider_kind,p.base_url,c.secret_envelope,m.input_price_micros,m.output_price_micros,m.priority,
        m.enabled AS model_enabled,p.enabled AS provider_enabled,p.settings_json,
        COALESCE(s.endpoint_mappings_json,'{"version":1}') AS endpoint_mappings_json,
        COALESCE(s.model_rules_json,'{"version":1}') AS model_rules_json,
        COALESCE(s.parameter_overrides_json,'{"version":1}') AS parameter_overrides_json,
        COALESCE(s.retry_statuses_json,'{"version":1}') AS retry_statuses_json,
        h.disabled_until,h.backoff_until,
        EXISTS(SELECT 1 FROM provider_quota_snapshots q WHERE q.remaining_micros<=0 AND (q.period_end IS NULL OR q.period_end>unixepoch())
          AND q.provider_id=p.id AND (q.id=(SELECT qq.id FROM provider_quota_snapshots qq WHERE qq.provider_id=p.id AND qq.credential_id IS NULL ORDER BY qq.collected_at DESC,qq.id LIMIT 1)
            OR q.id=(SELECT qq.id FROM provider_quota_snapshots qq WHERE qq.provider_id=p.id AND qq.credential_id=c.id ORDER BY qq.collected_at DESC,qq.id LIMIT 1))) AS quota_exhausted
        FROM models m JOIN providers p ON p.id=m.provider_id
        JOIN channel_credentials c ON c.id=(SELECT cc.id FROM channel_credentials cc WHERE cc.provider_id=p.id AND cc.enabled=1
          AND NOT EXISTS(SELECT 1 FROM provider_quota_snapshots cq WHERE cq.id=(SELECT latest.id FROM provider_quota_snapshots latest WHERE latest.provider_id=p.id AND latest.credential_id=cc.id ORDER BY latest.collected_at DESC,latest.id LIMIT 1)
            AND cq.remaining_micros<=0 AND (cq.period_end IS NULL OR cq.period_end>unixepoch())) ORDER BY cc.priority,cc.id LIMIT 1)
        LEFT JOIN channel_settings s ON s.provider_id=p.id LEFT JOIN channel_health_state h ON h.provider_id=p.id
        WHERE p.project_id=? ORDER BY m.priority,p.id,m.id
    "#,vec![key.project_id.clone().into()])).all(db).await?;
    let associations = Association::find_by_statement(statement("SELECT id,model_id,provider_id,match_type,pattern,conditions_json,priority,weight FROM model_associations WHERE project_id=? AND enabled=1 ORDER BY priority,id",vec![key.project_id.clone().into()])).all(db).await?;
    let mut result = vec![];
    for row in rows {
        let settings = policy::document(&row.settings_json)?;
        let tags: Vec<String> =
            serde_json::from_value(settings.get("tags").cloned().unwrap_or(json!([])))
                .map_err(|_| Error::Configuration)?;
        let mut rank = (row.priority, 1);
        let mut matched = row.public_name == model;
        let mut association_matched = false;
        for association in &associations {
            if association
                .provider_id
                .as_deref()
                .is_some_and(|id| id != row.provider_id)
            {
                continue;
            }
            if association
                .model_id
                .as_deref()
                .is_some_and(|id| id != row.model_id)
            {
                continue;
            }
            let matches = match association.match_type.as_str() {
                "exact" => association.pattern == model,
                "regex" => policy::regex(&association.pattern)?.is_match(model),
                "tag" => {
                    tags.contains(&association.pattern)
                        && (association.model_id.is_some() || row.public_name == model)
                }
                _ => return Err(Error::Configuration),
            };
            if matches
                && policy::matches(&policy::document(&association.conditions_json)?, context)?
            {
                if !association_matched || association.priority < rank.0 {
                    rank = (association.priority, association.weight as u32);
                }
                matched = true;
                association_matched = true;
                decisions.push(Decision {
                    stage: "association",
                    candidate: Some(association.id.clone()),
                    reason: "matched",
                });
            }
        }
        if !matched {
            continue;
        }
        let id = format!("{}:{}", row.provider_id, row.model_id);
        let reject = if !row.model_enabled || !row.provider_enabled {
            Some("disabled")
        } else if !routing.allows_tags(&tags) {
            Some("tag_policy")
        } else if row.disabled_until.is_some_and(|n| n > db::now())
            || row.backoff_until.is_some_and(|n| n > db::now())
        {
            Some("health_backoff")
        } else if row.quota_exhausted {
            Some("quota_exhausted")
        } else {
            None
        };
        if let Some(reason) = reject {
            decisions.push(Decision {
                stage: "eligibility",
                candidate: Some(id),
                reason,
            });
            continue;
        }
        let endpoint = context["endpoint"].as_str().ok_or(Error::Configuration)?;
        let capabilities: Vec<String> =
            serde_json::from_str(&row.capabilities).map_err(|_| Error::Configuration)?;
        if !supports(
            &row.provider_kind,
            &capabilities,
            endpoint,
            context["body"]["stream"].as_bool().unwrap_or(false),
        ) {
            decisions.push(Decision {
                stage: "endpoint",
                candidate: Some(id),
                reason: "unsupported_endpoint",
            });
            continue;
        }
        let model_rules = policy::document(&row.model_rules_json)?;
        if let Some(exclusions) = model_rules.get("exclude") {
            let mut excluded = false;
            for pattern in exclusions.as_array().ok_or(Error::Configuration)? {
                excluded |= policy::regex(pattern.as_str().ok_or(Error::Configuration)?)?
                    .is_match(&row.upstream_name);
            }
            if excluded {
                decisions.push(Decision {
                    stage: "eligibility",
                    candidate: Some(id),
                    reason: "model_excluded",
                });
                continue;
            }
        }
        let upstream_name = resolve_model(&row.upstream_name, &model_rules)?;
        if model_rules.get("stream").and_then(Value::as_bool) == Some(false)
            && context["body"]["stream"] == true
        {
            decisions.push(Decision {
                stage: "endpoint",
                candidate: Some(id),
                reason: "stream_disabled",
            });
            continue;
        }
        let limits: Limits =
            serde_json::from_value(settings.get("limits").cloned().unwrap_or(json!({})))
                .map_err(|_| Error::Configuration)?;
        let circuit: CircuitPolicy =
            serde_json::from_value(settings.get("circuit").cloned().unwrap_or(json!({})))
                .map_err(|_| Error::Configuration)?;
        if circuit.failures == 0 || circuit.window_ms == 0 || circuit.recovery_ms == 0 {
            return Err(Error::Configuration);
        }
        let mappings = policy::document(&row.endpoint_mappings_json)?;
        let path = mappings
            .get("paths")
            .and_then(|v| v.get(endpoint))
            .and_then(Value::as_str)
            .unwrap_or(endpoint)
            .to_owned();
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#']) {
            return Err(Error::Configuration);
        }
        result.push(Candidate {
            target: RouteTarget {
                public_name: row.public_name,
                upstream_name,
                provider_name: row.provider_name,
                provider_kind: row.provider_kind,
                base_url: row.base_url,
                secret_envelope: row.secret_envelope,
                input_price_micros: row.input_price_micros,
                output_price_micros: row.output_price_micros,
            },
            provider_id: row.provider_id,
            model_id: row.model_id,
            credential_id: row.credential_id,
            priority: rank.0,
            weight: rank.1,
            limits,
            retry: Retry::parse(&row.retry_statuses_json)?,
            circuit,
            overrides: row.parameter_overrides_json,
            model_rules,
            pass_user_agent: settings
                .get("pass_user_agent")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            endpoint: path,
            protocol_endpoint: endpoint.into(),
        });
    }
    result.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(a.id().cmp(&b.id()))
            .then(a.model_id.cmp(&b.model_id))
    });
    let mut seen = BTreeSet::new();
    result.retain(|c| {
        let unique = seen.insert(c.id());
        if !unique {
            decisions.push(Decision {
                stage: "dedup",
                candidate: Some(c.id()),
                reason: "duplicate_upstream_model",
            });
        }
        unique
    });
    Ok(result)
}

fn resolve_model(original: &str, rules: &Value) -> Result<String> {
    let mut name = original.to_owned();
    if let Some(prefix) = rules.get("strip_prefix") {
        name = name
            .strip_prefix(prefix.as_str().ok_or(Error::Configuration)?)
            .unwrap_or(&name)
            .to_owned();
    }
    if rules.get("lowercase").and_then(Value::as_bool) == Some(true) {
        name = name.to_lowercase();
    }
    if let Some(mapped) = rules.get("mappings").and_then(|map| map.get(&name)) {
        name = mapped.as_str().ok_or(Error::Configuration)?.to_owned();
    }
    if let Some(prefix) = rules.get("prefix") {
        name = format!("{}{name}", prefix.as_str().ok_or(Error::Configuration)?);
    }
    if name.is_empty() {
        return Err(Error::Configuration);
    }
    Ok(name)
}

fn supports(kind: &str, capabilities: &[String], endpoint: &str, stream: bool) -> bool {
    crate::providers::supports(kind, capabilities, endpoint, stream)
}
