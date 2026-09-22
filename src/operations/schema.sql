ALTER TABLE api_keys ADD COLUMN log_level TEXT NOT NULL DEFAULT 'inherit' CHECK(log_level IN ('inherit','off','metadata','redacted_body','full_body'));
ALTER TABLE model_prices ADD COLUMN origin TEXT NOT NULL DEFAULT 'operator' CHECK(origin IN ('operator','legacy_snapshot'));
ALTER TABLE usage_logs ADD COLUMN settlement_kind TEXT NOT NULL DEFAULT 'reported';
ALTER TABLE usage_logs ADD COLUMN image_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_logs ADD COLUMN image_output_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_logs ADD COLUMN cache_savings_micros INTEGER NOT NULL DEFAULT 0 CHECK(cache_savings_micros>=0);
CREATE TABLE service_groups (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 name TEXT NOT NULL, tier TEXT NOT NULL DEFAULT 'standard', ratio_millionths INTEGER NOT NULL DEFAULT 1000000 CHECK(ratio_millionths BETWEEN 0 AND 100000000),
 enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)), UNIQUE(project_id,name), UNIQUE(id,project_id)
);
CREATE TABLE service_group_keys (api_key_id TEXT PRIMARY KEY,project_id TEXT NOT NULL,group_id TEXT NOT NULL,
 FOREIGN KEY(api_key_id,project_id) REFERENCES api_keys(id,project_id) ON DELETE CASCADE,
 FOREIGN KEY(group_id,project_id) REFERENCES service_groups(id,project_id) ON DELETE CASCADE);
CREATE TABLE service_group_channels (group_id TEXT NOT NULL REFERENCES service_groups(id) ON DELETE CASCADE,provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,PRIMARY KEY(group_id,provider_id));
CREATE TRIGGER service_group_channel_scope BEFORE INSERT ON service_group_channels
 WHEN (SELECT project_id FROM service_groups WHERE id=NEW.group_id)!=(SELECT project_id FROM providers WHERE id=NEW.provider_id)
 BEGIN SELECT RAISE(ABORT,'cross-project service group channel'); END;
CREATE TABLE request_facts (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id), api_key_id TEXT REFERENCES api_keys(id) ON DELETE SET NULL,
 profile_id TEXT REFERENCES api_key_profiles(id) ON DELETE SET NULL,user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
 log_level TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'running', started_at INTEGER NOT NULL,finished_at INTEGER,
 ratio_millionths INTEGER NOT NULL DEFAULT 1000000 CHECK(ratio_millionths>=0)
);
CREATE INDEX request_facts_scope ON request_facts(project_id,started_at);
CREATE TABLE execution_facts (
 id TEXT PRIMARY KEY,request_id TEXT NOT NULL REFERENCES request_facts(id) ON DELETE CASCADE,
 provider_id TEXT REFERENCES providers(id) ON DELETE SET NULL,model_id TEXT REFERENCES models(id) ON DELETE SET NULL,
 credential_id TEXT REFERENCES channel_credentials(id) ON DELETE SET NULL,
 attempt INTEGER NOT NULL CHECK(attempt>0),status TEXT NOT NULL DEFAULT 'running',
 contacted INTEGER NOT NULL DEFAULT 0 CHECK(contacted IN (0,1)),config_json TEXT NOT NULL DEFAULT '{}',
 price_json TEXT NOT NULL CHECK(json_valid(price_json)),reserved_micros INTEGER NOT NULL DEFAULT 0 CHECK(reserved_micros>=0),
 started_at INTEGER NOT NULL,finished_at INTEGER,UNIQUE(request_id,attempt)
);
CREATE INDEX execution_facts_open ON execution_facts(status,started_at);
CREATE TABLE provider_response_settlements (fingerprint TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,execution_id TEXT NOT NULL);
CREATE TABLE instance_restore_receipts (artifact_id TEXT PRIMARY KEY,digest TEXT NOT NULL,restored_at INTEGER NOT NULL);
CREATE TABLE credential_health_state (credential_id TEXT PRIMARY KEY REFERENCES channel_credentials(id) ON DELETE CASCADE,consecutive_failures INTEGER NOT NULL DEFAULT 0,disabled_until INTEGER,updated_at INTEGER NOT NULL);
CREATE TABLE request_contents (request_id TEXT PRIMARY KEY REFERENCES requests(id) ON DELETE CASCADE,request_json TEXT,response_json TEXT);
ALTER TABLE traces ADD COLUMN external_id TEXT;
CREATE UNIQUE INDEX traces_scoped_external ON traces(project_id,api_key_id,external_id);
CREATE TABLE operation_jobs (
 id TEXT PRIMARY KEY, project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,kind TEXT NOT NULL,
 operation_key TEXT NOT NULL UNIQUE,payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
 status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','succeeded','failed')),
 attempts INTEGER NOT NULL DEFAULT 0,max_attempts INTEGER NOT NULL DEFAULT 5 CHECK(max_attempts BETWEEN 1 AND 20),
 due_at INTEGER NOT NULL,lease_until INTEGER,owner TEXT,fence INTEGER NOT NULL DEFAULT 0,error_code TEXT,
 created_at INTEGER NOT NULL,finished_at INTEGER
);
CREATE INDEX operation_jobs_due ON operation_jobs(status,due_at,lease_until);
CREATE TABLE operation_schedules (
 id TEXT PRIMARY KEY,project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,kind TEXT NOT NULL,payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
 interval_secs INTEGER NOT NULL CHECK(interval_secs BETWEEN 30 AND 31536000),next_run_at INTEGER NOT NULL,
 revision INTEGER NOT NULL DEFAULT 1,enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),last_error TEXT
);
ALTER TABLE data_storage_configs ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE backup_configs ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
CREATE TABLE backup_artifacts (id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id),digest TEXT NOT NULL,manifest_json TEXT NOT NULL,envelope TEXT NOT NULL,created_at INTEGER NOT NULL);
CREATE TABLE backup_job_targets (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES operation_jobs(id) ON DELETE CASCADE,
 storage_id TEXT NOT NULL,revision INTEGER NOT NULL,snapshot_json TEXT NOT NULL,secret_envelope TEXT,object_key TEXT NOT NULL,
 status TEXT NOT NULL DEFAULT 'pending',error_code TEXT,byte_size INTEGER,UNIQUE(job_id,storage_id));
CREATE TABLE backup_restores (artifact_id TEXT NOT NULL,project_id TEXT NOT NULL REFERENCES projects(id),strategy TEXT NOT NULL,restored_at INTEGER NOT NULL,PRIMARY KEY(artifact_id,project_id,strategy));
CREATE TABLE session_summaries (id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id),api_key_id TEXT NOT NULL REFERENCES api_keys(id),
 history_hash TEXT NOT NULL,covered_items INTEGER NOT NULL,model TEXT NOT NULL,summary_envelope TEXT NOT NULL,usage_json TEXT NOT NULL,created_at INTEGER NOT NULL,UNIQUE(api_key_id,history_hash,model));
CREATE TRIGGER immutable_price_update BEFORE UPDATE ON model_prices BEGIN SELECT RAISE(ABORT,'price versions are immutable'); END;
CREATE TRIGGER immutable_used_price_delete BEFORE DELETE ON model_prices WHEN EXISTS(SELECT 1 FROM usage_logs WHERE price_id=OLD.id) BEGIN SELECT RAISE(ABORT,'used price versions cannot be deleted'); END;
CREATE TRIGGER immutable_used_component_delete BEFORE DELETE ON model_price_components WHEN EXISTS(SELECT 1 FROM usage_cost_items WHERE price_component_id=OLD.id) BEGIN SELECT RAISE(ABORT,'used price components cannot be deleted'); END;
CREATE TRIGGER immutable_price_component_update BEFORE UPDATE ON model_price_components BEGIN SELECT RAISE(ABORT,'price components are immutable'); END;
CREATE TRIGGER immutable_summary_update BEFORE UPDATE ON session_summaries BEGIN SELECT RAISE(ABORT,'summaries are immutable'); END;
CREATE TRIGGER immutable_backup_update BEFORE UPDATE ON backup_artifacts BEGIN SELECT RAISE(ABORT,'backup artifacts are immutable'); END;
CREATE TRIGGER immutable_execution_snapshot BEFORE UPDATE OF price_json,config_json ON execution_facts
 WHEN OLD.config_json!='{}' BEGIN SELECT RAISE(ABORT,'execution snapshots are immutable'); END;
