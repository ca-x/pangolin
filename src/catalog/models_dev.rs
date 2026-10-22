use super::{Catalog, Model, Provider, types};
use crate::api::ApiError;
use models_dev_rs::{Catalog as UpstreamCatalog, Price};
use serde_json::json;

pub(super) const URL: &str = "https://models.dev/api.json";
const STANDARD_DOWNLOAD_BYTES: usize = 4 * 1024 * 1024;

pub(super) fn is_source(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| url.as_str() == URL)
}

pub(super) fn download_limit(url: &str) -> usize {
    if is_source(url) {
        models_dev_rs::DEFAULT_MAX_BYTES
    } else {
        STANDARD_DOWNLOAD_BYTES
    }
}

pub(super) fn normalize(bytes: &[u8]) -> Result<Vec<u8>, ApiError> {
    let upstream = UpstreamCatalog::from_slice(bytes)
        .map_err(|_| types::invalid("invalid models.dev catalog document"))?;
    if upstream.providers.is_empty()
        || upstream
            .providers
            .values()
            .all(|provider| provider.models.is_empty())
    {
        return Err(types::invalid("models.dev catalog contains no models"));
    }
    let revision = blake3::hash(bytes).to_hex().to_string();
    let source = types::Provenance {
        id: "models.dev".into(),
        url: URL.into(),
        revision: revision.clone(),
        license: "MIT".into(),
    };
    let mut catalog = Catalog {
        schema_version: 1,
        version: format!("models.dev-{}", &revision[..20]),
        source: source.clone(),
        providers: Vec::with_capacity(upstream.providers.len()),
        models: vec![],
        extensions: Default::default(),
    };
    for (provider_id, provider) in upstream.providers {
        let template = super::builtin()
            .providers
            .iter()
            .find(|entry| entry.id == provider_id);
        let mut normalized_provider = template.cloned().unwrap_or_else(|| Provider {
            id: provider_id.clone(),
            name: provider.name.clone(),
            category: "catalog".into(),
            default_base_url: None,
            auth_strategy: "unsupported".into(),
            adapter_kind: None,
            adapter_available: false,
            default_endpoints: vec![],
            model_discovery: Default::default(),
            quota: Default::default(),
            logo_key: format!("initials:{provider_id}"),
            logo_fallback: provider_id
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(2)
                .collect::<String>()
                .to_uppercase(),
            brand_color: None,
            monochrome: true,
            source: source.clone(),
            extensions: Default::default(),
        });
        normalized_provider.name = provider.name;
        normalized_provider.source = source.clone();
        normalized_provider.extensions.insert(
            "models_dev".into(),
            json!({
                "npm":provider.npm,"api":provider.api,"documentation":provider.doc,
                "environment_variables":provider.env,"metadata":provider.extensions,
                "verification":"catalog-only"
            }),
        );
        for (model_id, model) in provider.models {
            let kind = match model.kind.as_ref().map(|kind| kind.as_str()) {
                None => "chat",
                Some("reranking") => "rerank",
                Some(kind) => kind,
            };
            let mut extensions = model.extensions;
            extensions.insert("family".into(), json!(model.family));
            extensions.insert("description".into(), json!(model.description));
            extensions.insert("reasoning_options".into(), json!(model.reasoning_options));
            extensions.insert("open_weights".into(), json!(model.open_weights));
            extensions.insert(
                "models_dev".into(),
                json!({
                    "canonical_model_id":model.canonical_model_id,"attachment":model.attachment,
                    "interleaved":model.interleaved,"provider":model.provider,
                    "experimental":model.experimental,"cost":model.cost,
                    "limit":model.limit.extensions,"modalities":model.modalities.extensions,
                    "verification":"catalog-only"
                }),
            );
            let protocols = normalized_provider
                .default_endpoints
                .iter()
                .filter(|endpoint| match kind {
                    "chat" => matches!(
                        endpoint.protocol.as_str(),
                        "chat" | "responses" | "messages" | "gemini"
                    ),
                    "embedding" => endpoint.protocol == "embeddings",
                    "rerank" => endpoint.protocol == "rerank",
                    "image" => endpoint.protocol == "images",
                    "video" => endpoint.protocol == "videos",
                    "speech" => endpoint.protocol == "speech",
                    "transcription" => endpoint.protocol == "transcriptions",
                    _ => false,
                })
                .map(|endpoint| endpoint.protocol.clone())
                .collect();
            catalog.models.push(Model {
                id: format!("{provider_id}/{model_id}"),
                upstream_id: model_id.clone(),
                name: model.name,
                developer: provider_id.clone(),
                model_type: kind.into(),
                capabilities: types::Capabilities {
                    tools: model.tool_call,
                    reasoning: model.reasoning,
                    temperature: model.temperature,
                    json_schema: model.structured_output,
                    vision: Some(
                        model
                            .modalities
                            .input
                            .iter()
                            .any(|modality| modality.as_str() == "image"),
                    ),
                    caching: model.cost.as_ref().and_then(|cost| {
                        (cost.cache_read.is_some() || cost.cache_write.is_some()).then_some(true)
                    }),
                    ..Default::default()
                },
                modalities: types::Modalities {
                    input: model
                        .modalities
                        .input
                        .iter()
                        .map(|modality| modality.as_str().into())
                        .collect(),
                    output: model
                        .modalities
                        .output
                        .iter()
                        .map(|modality| modality.as_str().into())
                        .collect(),
                },
                protocols,
                limits: types::Limits {
                    context: Some(model.limit.context),
                    input: model.limit.input,
                    output: Some(model.limit.output),
                    ..Default::default()
                },
                aliases: vec![model_id],
                lifecycle: model
                    .status
                    .map_or_else(|| "catalogued".into(), |status| status.as_str().into()),
                knowledge_cutoff: model.knowledge.map(|date| date.to_string()),
                release_date: model.release_date.map(|date| date.to_string()),
                last_updated: model.last_updated.map(|date| date.to_string()),
                cost_defaults: model.cost.as_ref().map_or_else(Default::default, |cost| {
                    types::Costs {
                        input: price(Some(&cost.input)),
                        output: price(Some(&cost.output)),
                        cache_read: price(cost.cache_read.as_ref()),
                        cache_write: price(cost.cache_write.as_ref()),
                        currency: Some("USD".into()),
                        unit: Some("per_million_tokens".into()),
                        ..Default::default()
                    }
                }),
                source: source.clone(),
                extensions,
            });
        }
        catalog.providers.push(normalized_provider);
    }
    catalog.validate()?;
    let serialized =
        serde_json::to_vec(&catalog).map_err(|error| ApiError::Internal(error.into()))?;
    if serialized.len() > types::MAX_BYTES {
        return Err(types::invalid(
            "normalized models.dev catalog exceeds 16 MiB",
        ));
    }
    Ok(serialized)
}

fn price(value: Option<&Price>) -> Option<f64> {
    value.and_then(|price| price.as_number().as_f64())
}
