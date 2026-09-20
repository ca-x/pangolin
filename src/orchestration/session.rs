use super::repository::statement;
use crate::{crypto::SecretBox, db, models::ApiKeyCredential};
use moka::sync::Cache;
use sea_orm::{ConnectionTrait, DatabaseConnection, FromQueryResult, TransactionTrait};
use serde_json::{Value, json};
use std::time::Duration;

use super::{Error, Result};

const MAX_RECORD_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
struct Record {
    history: Vec<Value>,
    expires_at: i64,
    revision: String,
}
pub struct Sessions {
    records: Cache<String, Record>,
}

impl Default for Sessions {
    fn default() -> Self {
        Self {
            records: Cache::builder()
                .max_capacity(32 * 1024 * 1024)
                .weigher(|key: &String, value: &Record| {
                    (key.len()
                        + serde_json::to_vec(&value.history).map_or(MAX_RECORD_BYTES, |v| v.len()))
                        as u32
                })
                .time_to_live(Duration::from_secs(1800))
                .build(),
        }
    }
}

fn key(scope: &str, response: &str) -> String {
    format!("{}:{scope}:{response}", scope.len())
}

pub fn input(body: &Value) -> Result<Vec<Value>> {
    match body.get("input") {
        None => Ok(vec![]),
        Some(Value::String(text)) => {
            Ok(vec![json!({"type":"message","role":"user","content":text})])
        }
        Some(Value::Array(items)) => Ok(items.clone()),
        _ => Err(Error::Invalid("Responses input must be text or an array")),
    }
}

fn normalize(items: &mut [Value]) {
    for item in items {
        if let Some(object) = item.as_object_mut() {
            object.remove("status");
        }
    }
}

impl Sessions {
    pub fn clear(&self) {
        self.records.invalidate_all();
    }
    pub fn prepare(&self, scope: &str, body: &mut Value) -> Result<()> {
        let previous = body.get("previous_response_id").and_then(Value::as_str);
        if let Some(previous) = previous {
            // A missing record is explicit: forwarding an unscoped upstream response ID
            // could expose another tenant's history on a shared provider credential.
            let mut history = self
                .records
                .get(&key(scope, previous))
                .filter(|r| r.expires_at > db::now())
                .ok_or(Error::Invalid(
                    "previous response is unavailable in this API-key scope",
                ))?
                .history;
            history.extend(input(body)?);
            normalize(&mut history);
            body["input"] = Value::Array(history);
            body.as_object_mut()
                .ok_or(Error::Invalid("request must be an object"))?
                .remove("previous_response_id");
        } else if let Some(Value::Array(items)) = body.get_mut("input") {
            normalize(items);
        }
        Ok(())
    }

    pub fn record(&self, scope: &str, request: &Value, response: &Value) {
        if scope.is_empty() || response.get("status").and_then(Value::as_str) != Some("completed") {
            return;
        }
        let Some(id) = response.get("id").and_then(Value::as_str) else {
            return;
        };
        let Some(output) = response.get("output").and_then(Value::as_array) else {
            return;
        };
        let Ok(mut history) = input(request) else {
            return;
        };
        history.extend(output.clone());
        normalize(&mut history);
        if serde_json::to_vec(&history).is_ok_and(|v| v.len() <= MAX_RECORD_BYTES) {
            self.records.insert(
                key(scope, id),
                Record {
                    history,
                    expires_at: db::now() + 1800,
                    revision: String::new(),
                },
            );
        }
    }

    pub async fn restore(
        &self,
        db: &DatabaseConnection,
        secrets: &SecretBox,
        credential: &ApiKeyCredential,
        body: &mut Value,
    ) -> Result<()> {
        let scope = format!("{}:{}", credential.project_id, credential.id);
        if let Some(previous) = body.get("previous_response_id").and_then(Value::as_str) {
            #[derive(FromQueryResult)]
            struct Row {
                state_envelope: String,
                expires_at: i64,
            }
            // SQLite remains authoritative even on cache hits (expiry, deletion and ownership).
            let row=Row::find_by_statement(statement("SELECT state_envelope,expires_at FROM response_sessions WHERE project_id=? AND api_key_id=? AND response_id=? AND expires_at>? AND version=1",vec![credential.project_id.clone().into(),credential.id.clone().into(),previous.into(),db::now().into()])).one(db).await?
                .ok_or(Error::Invalid("previous response is unavailable in this API-key scope"))?;
            let revision = blake3::hash(row.state_envelope.as_bytes())
                .to_hex()
                .to_string();
            if self
                .records
                .get(&key(&scope, previous))
                .is_none_or(|record| record.revision != revision)
            {
                let plaintext = secrets
                    .decrypt(&row.state_envelope)
                    .map_err(|_| Error::Configuration)?;
                let state: Value =
                    serde_json::from_str(&plaintext).map_err(|_| Error::Configuration)?;
                if state["version"] != 1
                    || state["project_id"] != credential.project_id
                    || state["api_key_id"] != credential.id
                    || state["response_id"] != previous
                {
                    return Err(Error::Configuration);
                }
                let history = state["history"]
                    .as_array()
                    .ok_or(Error::Configuration)?
                    .clone();
                self.records.insert(
                    key(&scope, previous),
                    Record {
                        history,
                        expires_at: row.expires_at,
                        revision,
                    },
                );
            }
        }
        self.prepare(&scope, body)
    }

    pub async fn persist(
        &self,
        db: &DatabaseConnection,
        secrets: &SecretBox,
        credential: &ApiKeyCredential,
        request: &Value,
        response: &Value,
    ) -> Result<()> {
        let scope = format!("{}:{}", credential.project_id, credential.id);
        if response["status"] != "completed" {
            return Ok(());
        }
        let Some(id) = response.get("id").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(output) = response.get("output").and_then(Value::as_array) else {
            return Ok(());
        };
        let mut history = input(request)?;
        history.extend(output.clone());
        normalize(&mut history);
        let encoded = json!({"version":1,"project_id":credential.project_id,"api_key_id":credential.id,"response_id":id,"history":history}).to_string();
        if encoded.len() > MAX_RECORD_BYTES {
            return Ok(());
        }
        let envelope = secrets
            .encrypt(&encoded)
            .map_err(|_| Error::Configuration)?;
        let now = db::now();
        let transaction = db.begin().await?;
        transaction
            .execute(statement(
                "DELETE FROM response_sessions WHERE expires_at<=?",
                vec![now.into()],
            ))
            .await?;
        transaction.execute(statement("INSERT INTO response_sessions(id,project_id,api_key_id,response_id,state_envelope,version,updated_at,expires_at) VALUES(?,?,?,?,?,1,?,?) ON CONFLICT(api_key_id,response_id) DO UPDATE SET state_envelope=excluded.state_envelope,updated_at=excluded.updated_at,expires_at=excluded.expires_at",vec![uuid::Uuid::new_v4().to_string().into(),credential.project_id.clone().into(),credential.id.clone().into(),id.into(),envelope.into(),now.into(),(now+1800).into()])).await?;
        transaction.execute(statement("DELETE FROM response_sessions WHERE id IN (SELECT id FROM response_sessions WHERE api_key_id=? ORDER BY updated_at DESC,id DESC LIMIT -1 OFFSET 128)",vec![credential.id.clone().into()])).await?;
        transaction.execute(statement("DELETE FROM response_sessions WHERE id IN (SELECT id FROM (SELECT id,SUM(length(state_envelope)) OVER (ORDER BY updated_at DESC,id DESC) AS bytes,ROW_NUMBER() OVER (ORDER BY updated_at DESC,id DESC) AS position FROM response_sessions) WHERE bytes>268435456 OR position>10000)",vec![])).await?;
        transaction.commit().await?;
        self.record(&scope, request, response);
        Ok(())
    }
}
