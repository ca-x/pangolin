use super::*;
use crate::{api::ApiError, db};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use refresh::{Download, Transport};
use serde_json::json;
use std::{collections::VecDeque, future::Future, pin::Pin, sync::Mutex};

fn document(version: &str, name: &str) -> Catalog {
    let mut doc = builtin().clone();
    doc.version = version.into();
    doc.providers.retain(|p| p.id == "openai");
    doc.providers[0].name = name.into();
    doc.models.truncate(1);
    doc
}
fn encoded(doc: &Catalog) -> Vec<u8> {
    serde_json::to_vec(doc).unwrap()
}
fn source_input(priority: i32) -> repository::SourceInput {
    repository::SourceInput {
        name: format!("source-{priority}"),
        url: "https://catalog.example.com/catalog.json".into(),
        priority,
        refresh_interval_secs: 3600,
        enabled: true,
        signature_policy: "optional".into(),
        public_key: None,
    }
}

struct Mock {
    responses: Mutex<VecDeque<Download>>,
    requests: Mutex<Vec<(Option<String>, Option<String>)>>,
}
impl Mock {
    fn new(responses: Vec<Download>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(vec![]),
        }
    }
}
impl Transport for Mock {
    fn fetch<'a>(
        &'a self,
        source: &'a repository::Source,
    ) -> Pin<Box<dyn Future<Output = Result<Download, ApiError>> + Send + 'a>> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push((source.etag.clone(), source.last_modified.clone()));
            Ok(self.responses.lock().unwrap().pop_front().unwrap())
        })
    }
}
fn download(body: Vec<u8>) -> Download {
    Download {
        status: 200,
        body,
        etag: Some("\"catalog-v1\"".into()),
        last_modified: Some("Sun, 20 Sep 2026 00:00:00 GMT".into()),
        signature: None,
    }
}

#[test]
fn offline_bundle_contains_real_provider_presets_model_cards_and_logo_fallbacks() {
    let catalog = builtin();
    assert!(catalog.providers.len() >= 50);
    assert!(catalog.models.len() >= 400);
    for id in [
        "openai",
        "anthropic",
        "gemini",
        "azure",
        "bedrock",
        "vertex",
        "openrouter",
        "deepseek",
        "moonshot",
        "zhipu",
        "zai",
        "doubao",
        "alibaba",
        "xai",
        "groq",
        "mistral",
        "cohere",
        "jina",
        "siliconflow",
        "fireworks",
        "cerebras",
        "ollama",
        "github",
        "copilot",
        "minimax",
        "nanogpt",
    ] {
        let provider = catalog.providers.iter().find(|p| p.id == id).unwrap();
        assert!(!provider.logo_key.is_empty());
        assert!(!provider.logo_fallback.is_empty());
    }
    assert!(catalog.models.iter().any(|m| m.developer == "openai"
        && m.upstream_id.contains("gpt-5")
        && m.limits.context.is_some()));
    assert!(
        catalog
            .models
            .iter()
            .any(|m| m.developer == "anthropic" && m.capabilities.reasoning == Some(true))
    );
    assert!(
        catalog
            .models
            .iter()
            .any(|m| m.developer == "google" && m.capabilities.vision == Some(true))
    );
    assert!(
        !catalog
            .providers
            .iter()
            .find(|p| p.id == "copilot")
            .unwrap()
            .adapter_available
    );
    assert!(Catalog::parse(&encoded(catalog)).is_ok());
}

#[tokio::test]
async fn merge_priority_local_override_and_import_export_are_lossless() {
    let db = db::connect("sqlite::memory:").await.unwrap();
    let lower = repository::create_source(&db, source_input(10))
        .await
        .unwrap();
    let higher = repository::create_source(&db, source_input(20))
        .await
        .unwrap();
    repository::activate(
        &db,
        &lower,
        &encoded(&document("lower", "Low priority")),
        None,
        None,
        false,
    )
    .await
    .unwrap();
    repository::activate(
        &db,
        &higher,
        &encoded(&document("higher", "High priority")),
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let effective = repository::effective(&db).await.unwrap();
    assert_eq!(
        effective
            .providers
            .iter()
            .find(|p| p.id == "openai")
            .unwrap()
            .name,
        "High priority"
    );
    let mut local = document("local", "Local admin");
    local.models[0].capabilities.extra.insert(
        "future_sensor".into(),
        json!({"supported":true,"units":"widgets"}),
    );
    local.models[0].id = "local/custom".into();
    repository::import(&db, &encoded(&local)).await.unwrap();
    let updated = repository::source(&db, &higher.id).await.unwrap();
    repository::activate(
        &db,
        &updated,
        &encoded(&document("new", "Subscription update")),
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let effective = repository::effective(&db).await.unwrap();
    assert_eq!(
        effective
            .providers
            .iter()
            .find(|p| p.id == "openai")
            .unwrap()
            .name,
        "Local admin"
    );
    assert!(effective.models.iter().any(|m| m.id == "local/custom"));
    let other = db::connect("sqlite::memory:").await.unwrap();
    repository::import(&other, &encoded(&effective))
        .await
        .unwrap();
    let exported = repository::effective(&other).await.unwrap();
    assert_eq!(
        serde_json::to_value(&effective.providers).unwrap(),
        serde_json::to_value(&exported.providers).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&effective.models).unwrap(),
        serde_json::to_value(&exported.models).unwrap()
    );
    assert_eq!(
        exported
            .models
            .iter()
            .find(|m| m.id == "local/custom")
            .unwrap()
            .capabilities
            .extra["future_sensor"],
        json!({"supported":true,"units":"widgets"})
    );
}

#[tokio::test]
async fn conditional_refresh_and_failed_staging_preserve_last_known_good() {
    let db = db::connect("sqlite::memory:").await.unwrap();
    let source = repository::create_source(&db, source_input(1))
        .await
        .unwrap();
    let mut not_modified = download(vec![]);
    not_modified.status = 304;
    let transport = Mock::new(vec![
        download(encoded(&document("v1", "First"))),
        not_modified,
        download(b"{\"schema_version\":999}".to_vec()),
        download(vec![b'x'; types::MAX_BYTES + 1]),
    ]);
    let first = refresh::refresh_with(&db, &source.id, &transport)
        .await
        .unwrap();
    assert_eq!(
        refresh::refresh_with(&db, &source.id, &transport)
            .await
            .unwrap()
            .status,
        "not_modified"
    );
    assert_eq!(
        transport.requests.lock().unwrap()[1].0.as_deref(),
        Some("\"catalog-v1\"")
    );
    assert!(transport.requests.lock().unwrap()[1].1.is_some());
    for _ in 0..2 {
        assert!(
            refresh::refresh_with(&db, &source.id, &transport)
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
    }
    assert_eq!(
        repository::snapshots(&db, &source.id).await.unwrap().len(),
        1
    );
    assert_eq!(
        repository::effective(&db)
            .await
            .unwrap()
            .providers
            .iter()
            .find(|p| p.id == "openai")
            .unwrap()
            .name,
        "First"
    );
}

#[tokio::test]
async fn pinned_ed25519_signatures_rollback_and_revision_fencing() {
    let db = db::connect("sqlite::memory:").await.unwrap();
    let signing = SigningKey::from_bytes(&[7; 32]);
    let mut input = source_input(5);
    input.signature_policy = "required".into();
    input.public_key = Some(STANDARD.encode(signing.verifying_key().as_bytes()));
    let source = repository::create_source(&db, input).await.unwrap();
    let mut good = download(encoded(&document("signed-v1", "Signed")));
    good.signature = Some(STANDARD.encode(signing.sign(&good.body).to_bytes()));
    let mut bad = download(encoded(&document("tampered", "Bad")));
    bad.signature = good.signature.clone();
    let transport = Mock::new(vec![
        good,
        bad,
        download(encoded(&document("unsigned", "Unsigned"))),
    ]);
    let first = refresh::refresh_with(&db, &source.id, &transport)
        .await
        .unwrap();
    for _ in 0..2 {
        assert!(
            refresh::refresh_with(&db, &source.id, &transport)
                .await
                .is_err()
        );
    }
    assert!(
        repository::snapshots(&db, &source.id).await.unwrap()[0]["signature_verified"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        repository::source(&db, &source.id)
            .await
            .unwrap()
            .active_snapshot_id,
        first.snapshot_id
    );
    assert!(
        repository::activate(
            &db,
            &source,
            &encoded(&document("stale", "Stale")),
            None,
            None,
            true
        )
        .await
        .is_err()
    );
    let current = repository::source(&db, &source.id).await.unwrap();
    let second = repository::activate(
        &db,
        &current,
        &encoded(&document("v2", "Second")),
        None,
        None,
        true,
    )
    .await
    .unwrap();
    let current = repository::source(&db, &source.id).await.unwrap();
    repository::rollback(
        &db,
        &source.id,
        first.snapshot_id.as_ref().unwrap(),
        current.revision,
    )
    .await
    .unwrap();
    assert_eq!(
        repository::source(&db, &source.id)
            .await
            .unwrap()
            .previous_snapshot_id,
        Some(second)
    );
    repository::delete_source(&db, &source.id).await.unwrap();
    assert!(repository::sources(&db).await.unwrap().is_empty());
}

#[test]
fn subscriptions_reject_private_networks_non_https_and_unsafe_endpoint_shapes() {
    for url in [
        "http://catalog.example.com/a",
        "https://127.0.0.1/a",
        "https://[::1]/a",
        "https://10.0.0.1/a",
        "https://metadata.google.internal:8080/a",
        "https://name:password@example.com/a",
        "https://example.com/a#fragment",
    ] {
        assert!(refresh::source_url(url).is_err(), "{url}");
    }
    for ip in [
        "127.0.0.1",
        "169.254.169.254",
        "10.0.0.1",
        "192.168.1.1",
        "100.64.0.1",
        "::1",
        "fc00::1",
        "::ffff:127.0.0.1",
        "2001:db8::1",
    ] {
        assert!(!refresh::public_address(ip.parse().unwrap()), "{ip}");
    }
    assert!(refresh::public_address("8.8.8.8".parse().unwrap()));
    assert!(refresh::public_address("2606:4700::1111".parse().unwrap()));
    let mut doc = document("bad", "Bad");
    doc.providers[0].default_endpoints[0].transport = types::Transport::Websocket;
    assert!(doc.validate().is_err());
    let mut doc = document("bad", "Bad");
    doc.providers[0].logo_key = "https://malicious.invalid/a.svg".into();
    assert!(doc.validate().is_err());
}
