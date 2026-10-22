use super::*;
use crate::{api::ApiError, db};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use std::{collections::VecDeque, future::Future, pin::Pin, sync::Mutex};

fn fixture() -> Value {
    json!({"openai":{
        "id":"openai","name":"OpenAI","env":["OPENAI_API_KEY"],
        "npm":"@ai-sdk/openai","api":"https://untrusted.example/v9","doc":"https://openai.com/docs",
        "models":{"test-model":{
            "id":"test-model","name":"Test model","description":"Reference metadata",
            "family":"test","tool_call":false,"reasoning":true,"temperature":false,
            "structured_output":true,"knowledge":"2025-07","release_date":"2025-09-29",
            "last_updated":"2026-10-01","modalities":{"input":["text","image"],"output":["text"]},
            "limit":{"context":200000,"input":180000,"output":20000},
            "cost":{"input":0.125,"output":0,"cache_read":0.01,
                "context_over_200k":{"input":0.25,"output":1}},
            "future_fact":{"enabled":true}
        }}
    }})
}

#[test]
fn models_dev_metadata_retains_source_unknowns_and_trusted_adapters() {
    let bytes = serde_json::to_vec(&fixture()).unwrap();
    let catalog = Catalog::parse(&models_dev::normalize(&bytes).unwrap()).unwrap();
    let model = &catalog.models[0];
    assert_eq!(model.id, "openai/test-model");
    assert_eq!(model.capabilities.tools, Some(false));
    assert_eq!(model.capabilities.streaming, None);
    assert_eq!(model.capabilities.reasoning, Some(true));
    assert_eq!(model.capabilities.vision, Some(true));
    assert_eq!(model.limits.input, Some(180000));
    assert_eq!(model.cost_defaults.input, Some(0.125));
    assert_eq!(model.cost_defaults.output, Some(0.0));
    assert_eq!(model.cost_defaults.currency.as_deref(), Some("USD"));
    assert_eq!(model.extensions["future_fact"], json!({"enabled":true}));
    assert_eq!(
        model.extensions["models_dev"]["cost"]["context_over_200k"]["input"],
        json!(0.25)
    );
    assert_eq!(
        model.source.revision,
        blake3::hash(&bytes).to_hex().to_string()
    );
    let builtin = builtin()
        .providers
        .iter()
        .find(|provider| provider.id == "openai")
        .unwrap();
    assert_eq!(
        catalog.providers[0].default_base_url,
        builtin.default_base_url
    );
    assert_eq!(catalog.providers[0].adapter_kind, builtin.adapter_kind);
    assert_eq!(catalog.providers[0].logo_key, builtin.logo_key);
}

#[test]
fn models_dev_unknown_providers_cannot_install_adapters_and_vendor_ids_survive() {
    let mut value = fixture();
    let mut provider = value.as_object_mut().unwrap().remove("openai").unwrap();
    provider["id"] = json!("future-provider");
    let model = provider["models"]
        .as_object_mut()
        .unwrap()
        .remove("test-model")
        .unwrap();
    provider["models"]["@cf/~test"] = model;
    provider["models"]["@cf/~test"]["id"] = json!("@cf/~test");
    provider["models"]["@cf/~test"]
        .as_object_mut()
        .unwrap()
        .remove("cost");
    provider["models"]["@cf/~test"]
        .as_object_mut()
        .unwrap()
        .remove("tool_call");
    value["future-provider"] = provider;
    let catalog =
        Catalog::parse(&models_dev::normalize(&serde_json::to_vec(&value).unwrap()).unwrap())
            .unwrap();
    assert!(!catalog.providers[0].adapter_available);
    assert_eq!(catalog.providers[0].adapter_kind, None);
    assert_eq!(catalog.providers[0].auth_strategy, "unsupported");
    assert!(catalog.models[0].protocols.is_empty());
    assert_eq!(catalog.models[0].upstream_id, "@cf/~test");
    assert_eq!(catalog.models[0].cost_defaults.input, None);
    assert_eq!(catalog.models[0].capabilities.tools, None);
}

#[test]
fn models_dev_only_exact_official_url_gets_native_format_and_larger_bound() {
    assert_eq!(
        models_dev::download_limit(models_dev::URL),
        16 * 1024 * 1024
    );
    for url in [
        "https://models.dev/api.json?other=1",
        "https://models.dev.evil.example/api.json",
        "https://example.com/api.json",
    ] {
        assert!(!models_dev::is_source(url));
        assert_eq!(models_dev::download_limit(url), 4 * 1024 * 1024);
    }
    for bytes in [b"{}".as_slice(), b"{\"openai\":{}}", b"[]"] {
        assert!(models_dev::normalize(bytes).is_err());
    }
    let mut value = fixture();
    value["openai"]["models"]["test-model"]["cost"]["input"] = json!(-1);
    assert!(models_dev::normalize(&serde_json::to_vec(&value).unwrap()).is_err());
}

struct Mock(Mutex<VecDeque<refresh::Download>>);

impl refresh::Transport for Mock {
    fn fetch<'a>(
        &'a self,
        _: &'a repository::Source,
    ) -> Pin<Box<dyn Future<Output = Result<refresh::Download, ApiError>> + Send + 'a>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().pop_front().unwrap()) })
    }
}

fn download(body: Vec<u8>) -> refresh::Download {
    refresh::Download {
        status: 200,
        body,
        etag: Some("\"test-v1\"".into()),
        last_modified: None,
        signature: None,
    }
}

fn source_input() -> repository::SourceInput {
    repository::SourceInput {
        name: "models.dev".into(),
        url: models_dev::URL.into(),
        priority: 50,
        refresh_interval_secs: 86400,
        enabled: true,
        signature_policy: "none".into(),
        public_key: None,
        proxy_preset_id: None,
    }
}

#[tokio::test]
async fn models_dev_concurrent_connects_reuse_source_without_overwriting_configuration() {
    let db = db::connect("sqlite::memory:").await.unwrap();
    let (first, second) = tokio::join!(
        repository::create_source(&db, source_input(), None),
        repository::create_source(&db, source_input(), None)
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(repository::sources(&db).await.unwrap().len(), 1);
    let mut edited = source_input();
    edited.name = "Operator subscription".into();
    edited.refresh_interval_secs = 3600;
    edited.enabled = false;
    repository::update_source(&db, &first.id, first.revision, edited, None)
        .await
        .unwrap();
    let mut canonical_alias = source_input();
    canonical_alias.url = "https://MODELS.DEV:443/api.json".into();
    let reused = repository::create_source(&db, canonical_alias, None)
        .await
        .unwrap();
    assert_eq!(reused.id, first.id);
    assert_eq!(reused.name, "Operator subscription");
    assert_eq!(reused.refresh_interval_secs, 3600);
    assert!(!reused.enabled);
    let mut custom = source_input();
    custom.url = "https://example.com/catalog.json".into();
    let custom = repository::create_source(&db, custom, None).await.unwrap();
    assert!(
        repository::update_source(&db, &custom.id, custom.revision, source_input(), None)
            .await
            .is_err()
    );
    assert_eq!(
        repository::source(&db, &custom.id).await.unwrap().url,
        "https://example.com/catalog.json"
    );
}

#[tokio::test]
async fn models_dev_refresh_is_conditional_atomic_and_retains_last_known_good() {
    let db = db::connect("sqlite::memory:").await.unwrap();
    let source = repository::create_source(
        &db,
        repository::SourceInput {
            name: "models.dev".into(),
            url: models_dev::URL.into(),
            priority: 50,
            refresh_interval_secs: 86400,
            enabled: true,
            signature_policy: "none".into(),
            public_key: None,
            proxy_preset_id: None,
        },
        None,
    )
    .await
    .unwrap();
    let mut not_modified = download(vec![]);
    not_modified.status = 304;
    let transport = Mock(Mutex::new(VecDeque::from([
        download(serde_json::to_vec(&fixture()).unwrap()),
        not_modified,
        download(b"{\"openai\":{}}".to_vec()),
    ])));
    let first = refresh::refresh_with(&db, &source.id, &transport, None)
        .await
        .unwrap();
    assert_eq!(first.status, "activated");
    assert!(
        repository::effective(&db)
            .await
            .unwrap()
            .models
            .iter()
            .any(|model| model.id == "openai/test-model")
    );
    let second = refresh::refresh_with(&db, &source.id, &transport, None)
        .await
        .unwrap();
    assert_eq!(second.status, "not_modified");
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert!(
        refresh::refresh_with(&db, &source.id, &transport, None)
            .await
            .is_err()
    );
    let failed = repository::source(&db, &source.id).await.unwrap();
    assert_eq!(failed.active_snapshot_id, first.snapshot_id);
    assert!(failed.last_error.is_some());
    assert!(failed.last_success_at.is_some());
    assert!(
        repository::effective(&db)
            .await
            .unwrap()
            .models
            .iter()
            .any(|model| model.id == "openai/test-model")
    );
}

#[tokio::test]
async fn models_dev_signatures_cover_original_bytes_and_local_overrides_win() {
    let db = db::connect("sqlite::memory:").await.unwrap();
    let key = SigningKey::from_bytes(&[41; 32]);
    let source = repository::create_source(
        &db,
        repository::SourceInput {
            name: "signed models.dev".into(),
            url: models_dev::URL.into(),
            priority: 50,
            refresh_interval_secs: 86400,
            enabled: true,
            signature_policy: "required".into(),
            public_key: Some(STANDARD.encode(key.verifying_key().as_bytes())),
            proxy_preset_id: None,
        },
        None,
    )
    .await
    .unwrap();
    let body = serde_json::to_vec(&fixture()).unwrap();
    let mut signed = download(body.clone());
    signed.signature = Some(STANDARD.encode(key.sign(&body).to_bytes()));
    let unsigned = download(body);
    let transport = Mock(Mutex::new(VecDeque::from([signed, unsigned])));
    let first = refresh::refresh_with(&db, &source.id, &transport, None)
        .await
        .unwrap();
    let mut local =
        Catalog::parse(&models_dev::normalize(&serde_json::to_vec(&fixture()).unwrap()).unwrap())
            .unwrap();
    local.models[0].name = "Operator model".into();
    local.models[0].cost_defaults.input = Some(9.0);
    local.providers.clear();
    repository::import(&db, &serde_json::to_vec(&local).unwrap(), None)
        .await
        .unwrap();
    assert!(
        refresh::refresh_with(&db, &source.id, &transport, None)
            .await
            .is_err()
    );
    assert_eq!(
        repository::source(&db, &source.id)
            .await
            .unwrap()
            .active_snapshot_id,
        first.snapshot_id
    );
    let effective = repository::effective(&db).await.unwrap();
    let model = effective
        .models
        .iter()
        .find(|model| model.id == "openai/test-model")
        .unwrap();
    assert_eq!(model.name, "Operator model");
    assert_eq!(model.cost_defaults.input, Some(9.0));
}
