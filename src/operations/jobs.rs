//! Durable jobs use atomic UPDATE RETURNING claims and monotonic fences.
use super::{id, sql};
use crate::{api::ApiError, db};
use sea_orm::{ConnectionTrait, DatabaseConnection, FromQueryResult, TransactionTrait};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, FromQueryResult, Serialize)]
pub struct Claim {
    pub id: String,
    pub project_id: Option<String>,
    pub kind: String,
    pub payload_json: String,
    pub fence: i64,
    pub owner: String,
    pub attempts: i64,
    pub lease_until: i64,
}
pub async fn fence(db: &impl ConnectionTrait, claim: &Claim) -> Result<(), ApiError> {
    let changed=db.execute(sql("UPDATE operation_jobs SET fence=fence WHERE id=? AND owner=? AND fence=? AND status='running' AND lease_until>?",vec![claim.id.clone().into(),claim.owner.clone().into(),claim.fence.into(),db::now().into()])).await?.rows_affected();
    if changed != 1 {
        return Err(ApiError::Conflict("operation lease expired".into()));
    }
    Ok(())
}
pub async fn enqueue(
    db: &impl ConnectionTrait,
    project: Option<&str>,
    kind: &str,
    key: &str,
    payload: &Value,
    due: i64,
) -> Result<String, ApiError> {
    if payload.to_string().len() > 4 * 1024 * 1024 || key.len() > 512 {
        return Err(ApiError::BadRequest("job payload exceeds limit".into()));
    }
    let row=db.query_one(sql("INSERT INTO operation_jobs(id,project_id,kind,operation_key,payload_json,due_at,created_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(operation_key) DO UPDATE SET operation_key=excluded.operation_key RETURNING id",vec![id().into(),project.into(),kind.into(),key.into(),payload.to_string().into(),due.into(),db::now().into()])).await?.ok_or_else(||ApiError::Internal(anyhow::anyhow!("job enqueue returned no row")))?;
    Ok(row.try_get("", "id")?)
}
pub async fn claim(
    db: &DatabaseConnection,
    owner: &str,
    now: i64,
    lease: i64,
) -> Result<Option<Claim>, ApiError> {
    if !(1..=3600).contains(&lease) || owner.is_empty() || owner.len() > 128 {
        return Err(ApiError::BadRequest("invalid lease".into()));
    }
    db.execute(sql("UPDATE operation_jobs SET status='failed',error_code='attempts_exhausted',finished_at=? WHERE attempts>=max_attempts AND (status='pending' OR status='running' AND lease_until<=?)",vec![now.into(),now.into()])).await?;
    Ok(Claim::find_by_statement(sql("UPDATE operation_jobs SET status='running',owner=?,lease_until=?,fence=fence+1,attempts=attempts+1 WHERE id=(SELECT id FROM operation_jobs WHERE attempts<max_attempts AND (status='pending' AND due_at<=? OR status='running' AND lease_until<=?) ORDER BY due_at,id LIMIT 1) RETURNING id,project_id,kind,payload_json,fence,owner,attempts,lease_until",vec![owner.into(),(now+lease).into(),now.into(),now.into()])).one(db).await?)
}
pub async fn heartbeat(
    db: &DatabaseConnection,
    claim: &Claim,
    now: i64,
    lease: i64,
) -> Result<bool, ApiError> {
    Ok(db.execute(sql("UPDATE operation_jobs SET lease_until=? WHERE id=? AND owner=? AND fence=? AND status='running' AND lease_until>?",vec![(now+lease).into(),claim.id.clone().into(),claim.owner.clone().into(),claim.fence.into(),now.into()])).await?.rows_affected()==1)
}
pub async fn finish(
    db: &DatabaseConnection,
    claim: &Claim,
    now: i64,
    success: bool,
) -> Result<bool, ApiError> {
    let delay = 2i64.pow(claim.attempts.min(10) as u32).min(3600);
    Ok(db.execute(sql("UPDATE operation_jobs SET status=CASE WHEN ? THEN 'succeeded' WHEN attempts>=max_attempts THEN 'failed' ELSE 'pending' END,finished_at=CASE WHEN ? OR attempts>=max_attempts THEN ? ELSE NULL END,error_code=CASE WHEN ? THEN NULL ELSE 'operation_failed' END,due_at=?,owner=NULL,lease_until=NULL WHERE id=? AND owner=? AND fence=? AND status='running' AND lease_until>?",vec![success.into(),success.into(),now.into(),success.into(),(now+delay).into(),claim.id.clone().into(),claim.owner.clone().into(),claim.fence.into(),now.into()])).await?.rows_affected()==1)
}
pub async fn enqueue_due(db: &DatabaseConnection, now: i64) -> Result<usize, ApiError> {
    let rows=db.query_all(sql("SELECT id,project_id,kind,payload_json,revision,next_run_at,interval_secs FROM operation_schedules WHERE enabled=1 AND next_run_at<=? ORDER BY next_run_at,id LIMIT 100",vec![now.into()])).await?;
    let mut count = 0;
    for row in rows {
        let schedule: String = row.try_get("", "id")?;
        let revision: i64 = row.try_get("", "revision")?;
        let slot: i64 = row.try_get("", "next_run_at")?;
        let interval: i64 = row.try_get("", "interval_secs")?;
        let project: Option<String> = row.try_get("", "project_id")?;
        let kind: String = row.try_get("", "kind")?;
        let tx = db.begin().await?;
        let changed=tx.execute(sql("UPDATE operation_schedules SET next_run_at=?,last_error=NULL WHERE id=? AND revision=? AND next_run_at=? AND enabled=1",vec![(now+interval).into(),schedule.clone().into(),revision.into(),slot.into()])).await?.rows_affected();
        if changed != 1 {
            tx.rollback().await?;
            continue;
        }
        let key = format!("schedule:{schedule}:{revision}:{slot}");
        let result:Result<(),ApiError>=async {
            let payload:Value=serde_json::from_str(&row.try_get::<String>("","payload_json")?).map_err(|_|ApiError::BadRequest("invalid schedule payload".into()))?;
            if kind=="automatic_backup" {
                let targets:Vec<String>=serde_json::from_value(payload["targets"].clone()).map_err(|_|ApiError::BadRequest("invalid scheduled backup targets".into()))?;
                let resources:Vec<String>=serde_json::from_value(payload["resources"].clone()).map_err(|_|ApiError::BadRequest("invalid scheduled backup resources".into()))?;
                let job=super::backup::enqueue_in(&tx,project.as_deref().ok_or(ApiError::Forbidden)?,&targets,&super::backup::Selection{resources},&key).await?;
                if let Some(keep)=payload["retention_count"].as_i64() {
                    if !(1..=1000).contains(&keep){return Err(ApiError::BadRequest("invalid backup retention".into()))}
                    tx.execute(sql("UPDATE operation_jobs SET payload_json=json_set(payload_json,'$.retention_count',?) WHERE id=?",vec![keep.into(),job.into()])).await?;
                }
            } else {enqueue(&tx,project.as_deref(),&kind,&key,&payload,now).await?;}
            Ok(())
        }.await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                count += 1;
            }
            Err(_) => {
                tx.rollback().await?;
                let tx = db.begin().await?;
                let changed=tx.execute(sql("UPDATE operation_schedules SET next_run_at=?,last_error='invalid_configuration' WHERE id=? AND revision=? AND next_run_at=?",vec![(now+interval).into(),schedule.clone().into(),revision.into(),slot.into()])).await?.rows_affected();
                if changed == 1 {
                    let job = enqueue(
                        &tx,
                        project.as_deref(),
                        "invalid_schedule",
                        &key,
                        &serde_json::json!({"schedule_id":schedule}),
                        now,
                    )
                    .await?;
                    tx.execute(sql("UPDATE operation_jobs SET status='failed',error_code='invalid_configuration',finished_at=? WHERE id=?",vec![now.into(),job.into()])).await?;
                }
                tx.commit().await?;
            }
        }
    }
    Ok(count)
}
