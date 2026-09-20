//! Immutable integer micro-USD prices, including cache accounting and marginal tiers.
use super::sql;
use crate::{api::ApiError, db, orchestration::Candidate};
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub cache_write_1h: i64,
    pub reasoning: i64,
    pub units: i64,
    #[serde(default)]
    pub image_input: i64,
    #[serde(default)]
    pub image_output: i64,
    #[serde(default)]
    pub reported: bool,
}
fn number(value: &Value, names: &[&str]) -> i64 {
    names
        .iter()
        .find_map(|name| value.pointer(name).and_then(Value::as_i64))
        .unwrap_or(0)
        .max(0)
}
impl Usage {
    pub fn parse(value: &Value) -> Self {
        Self::parse_for(
            value,
            matches!(
                value["type"].as_str(),
                Some("message" | "message_start" | "message_delta")
            ) || value.get("stop_reason").is_some(),
        )
    }
    pub fn parse_for(value: &Value, anthropic: bool) -> Self {
        let usage = [
            value.get("usage"),
            value.get("usageMetadata"),
            value.pointer("/response/usage"),
            value.pointer("/message/usage"),
        ]
        .into_iter()
        .flatten()
        .find(|value| value.is_object());
        let Some(v) = usage else {
            return Self::default();
        };
        let fields = [
            "/input_tokens",
            "/prompt_tokens",
            "/promptTokenCount",
            "/output_tokens",
            "/completion_tokens",
            "/candidatesTokenCount",
            "/totalTokenCount",
            "/cache_read_input_tokens",
            "/cache_creation_input_tokens",
            "/prompt_cache_hit_tokens",
            "/request_units",
        ];
        let supplied = fields
            .into_iter()
            .filter_map(|name| v.pointer(name))
            .collect::<Vec<_>>();
        let reported = !supplied.is_empty()
            && supplied
                .iter()
                .all(|value| value.as_i64().is_some_and(|n| n >= 0));
        let read = number(
            v,
            &[
                "/cache_read_input_tokens",
                "/input_tokens_details/cached_tokens",
                "/prompt_tokens_details/cached_tokens",
                "/cachedContentTokenCount",
                "/prompt_cache_hit_tokens",
            ],
        );
        let write = number(
            v,
            &[
                "/cache_creation_input_tokens",
                "/input_tokens_details/cache_write_tokens",
                "/prompt_tokens_details/cache_write_tokens",
                "/prompt_tokens_details/cache_creation_tokens",
            ],
        );
        let mut input = number(v, &["/input_tokens", "/prompt_tokens", "/promptTokenCount"]);
        // Anthropic reports uncached input separately; OpenAI and Gemini include cache hits.
        if anthropic {
            input = input.saturating_add(read).saturating_add(write);
        }
        let output = if v.get("totalTokenCount").is_some() {
            number(v, &["/totalTokenCount"]).saturating_sub(input)
        } else {
            number(
                v,
                &[
                    "/output_tokens",
                    "/completion_tokens",
                    "/candidatesTokenCount",
                ],
            )
            .saturating_add(number(v, &["/thoughtsTokenCount"]))
        };
        Self {
            input,
            output,
            cache_read: read,
            cache_write: write,
            cache_write_1h: number(v, &["/cache_creation/ephemeral_1h_input_tokens"]),
            reasoning: number(
                v,
                &[
                    "/output_tokens_details/reasoning_tokens",
                    "/completion_tokens_details/reasoning_tokens",
                    "/thoughtsTokenCount",
                ],
            ),
            units: number(v, &["/request_units"]),
            image_input: number(
                v,
                &[
                    "/input_tokens_details/image_tokens",
                    "/prompt_tokens_details/image_tokens",
                ],
            ),
            image_output: number(
                v,
                &[
                    "/output_tokens_details/image_tokens",
                    "/completion_tokens_details/image_tokens",
                ],
            ),
            reported,
        }
    }
    pub fn merge(&mut self, other: Self) {
        if !other.reported {
            return;
        }
        // Provider stream usage is cumulative, except Anthropic splits input/output fields.
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.cache_write = self.cache_write.max(other.cache_write);
        self.cache_write_1h = self.cache_write_1h.max(other.cache_write_1h);
        self.reasoning = self.reasoning.max(other.reasoning);
        self.units = self.units.max(other.units);
        self.image_input = self.image_input.max(other.image_input);
        self.image_output = self.image_output.max(other.image_output);
        self.reported = true;
    }
    pub fn merge_event(&mut self, value: &Value, anthropic: bool) {
        let parsed = Self::parse_for(value, anthropic);
        if anthropic && value["type"] == "message_delta" {
            let v = &value["usage"];
            let fresh = number(v, &["/input_tokens"]);
            let prior = self
                .input
                .saturating_sub(self.cache_read)
                .saturating_sub(self.cache_write);
            if fresh > 0 && (prior == 0 || fresh <= prior) {
                if v.get("cache_read_input_tokens").is_some() {
                    self.cache_read = parsed.cache_read;
                }
                if v.get("cache_creation_input_tokens").is_some() {
                    self.cache_write = parsed.cache_write;
                }
                self.input = fresh
                    .saturating_add(self.cache_read)
                    .saturating_add(self.cache_write);
                self.output = self.output.max(parsed.output);
                self.reported |= parsed.reported;
                return;
            }
            let mut output_only = parsed;
            output_only.input = self.input;
            output_only.cache_read = self.cache_read;
            output_only.cache_write = self.cache_write;
            self.merge(output_only);
            return;
        }
        self.merge(parsed);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub up_to: Option<i64>,
    pub unit_price_micros: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    pub unit_size: i64,
    pub unit_price_micros: i64,
    #[serde(default)]
    pub tiers: Vec<Tier>,
    #[serde(default)]
    pub cache_ttl: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Price {
    pub id: Option<String>,
    pub model_id: String,
    pub ratio_millionths: i64,
    pub components: Vec<Component>,
}
#[derive(Debug, Serialize)]
pub struct CostItem {
    pub component_id: String,
    pub kind: String,
    pub quantity: i64,
    pub subtotal_micros: i64,
}
fn invalid() -> ApiError {
    ApiError::BadRequest("invalid or overflowing price/usage configuration".into())
}
impl Price {
    pub fn validate(&self) -> Result<(), ApiError> {
        if !(0..=100_000_000).contains(&self.ratio_millionths) || self.components.len() > 64 {
            return Err(invalid());
        }
        let mut kinds = std::collections::BTreeSet::new();
        for c in &self.components {
            if !matches!(
                c.kind.as_str(),
                "input" | "output" | "cache_read" | "cache_write" | "reasoning" | "flat" | "unit"
            ) || c.unit_size <= 0
                || c.unit_price_micros < 0
                || c.tiers.len() > 64
                || !kinds.insert((&c.kind, &c.cache_ttl))
            {
                return Err(invalid());
            }
            if c.cache_ttl
                .as_deref()
                .is_some_and(|v| !matches!(v, "5m" | "1h") || c.kind != "cache_write")
            {
                return Err(invalid());
            }
            let mut last = 0;
            for (i, tier) in c.tiers.iter().enumerate() {
                if tier.unit_price_micros < 0
                    || tier.up_to.is_some_and(|v| v <= last)
                    || tier.up_to.is_none() && i + 1 != c.tiers.len()
                {
                    return Err(invalid());
                }
                last = tier.up_to.unwrap_or(i64::MAX)
            }
            if !c.tiers.is_empty() && c.tiers.last().unwrap().up_to.is_some() {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn calculate(&self, u: &Usage) -> Result<(i64, Vec<CostItem>), ApiError> {
        self.validate()?;
        if [
            u.input,
            u.output,
            u.cache_read,
            u.cache_write,
            u.cache_write_1h,
            u.reasoning,
            u.units,
        ]
        .iter()
        .any(|n| *n < 0)
            || u.cache_read.saturating_add(u.cache_write) > u.input
            || u.cache_write_1h > u.cache_write
            || u.reasoning > u.output
        {
            return Err(invalid());
        }
        let has = |kind: &str| self.components.iter().any(|c| c.kind == kind);
        let mut items = vec![];
        let mut total = 0i64;
        for c in &self.components {
            let quantity = match c.kind.as_str() {
                "input" => {
                    u.input
                        - if has("cache_read") { u.cache_read } else { 0 }
                        - if has("cache_write") { u.cache_write } else { 0 }
                }
                "output" => u.output - if has("reasoning") { u.reasoning } else { 0 },
                "cache_read" => u.cache_read,
                "cache_write" => match c.cache_ttl.as_deref() {
                    Some("1h") => u.cache_write_1h,
                    Some("5m") => u.cache_write - u.cache_write_1h,
                    _ => u.cache_write,
                },
                "reasoning" => u.reasoning,
                "flat" => 1,
                "unit" => u.units,
                _ => return Err(invalid()),
            };
            let mut numerator = 0i128;
            if c.tiers.is_empty() {
                numerator = i128::from(quantity) * i128::from(c.unit_price_micros)
            } else {
                let mut lower = 0;
                for tier in &c.tiers {
                    let upper = tier.up_to.unwrap_or(i64::MAX);
                    let amount = quantity.min(upper).saturating_sub(lower).max(0);
                    numerator = numerator
                        .checked_add(i128::from(amount) * i128::from(tier.unit_price_micros))
                        .ok_or_else(invalid)?;
                    lower = upper;
                }
            }
            let denominator = i128::from(c.unit_size) * 1_000_000;
            // Round each independently auditable item upwards to one micro-USD.
            let scaled = numerator
                .checked_mul(i128::from(self.ratio_millionths))
                .ok_or_else(invalid)?;
            let subtotal =
                i64::try_from((scaled + denominator - 1) / denominator).map_err(|_| invalid())?;
            total = total.checked_add(subtotal).ok_or_else(invalid)?;
            items.push(CostItem {
                component_id: c.id.clone(),
                kind: c.kind.clone(),
                quantity,
                subtotal_micros: subtotal,
            });
        }
        Ok((total, items))
    }
    pub fn upper_bound(
        &self,
        tokens: u32,
        payload: &Value,
        endpoint: &str,
    ) -> Result<i64, ApiError> {
        let media = crate::providers::is_media(endpoint);
        if media {
            if self
                .components
                .iter()
                .any(|c| !matches!(c.kind.as_str(), "flat" | "unit"))
                || self.components.is_empty()
            {
                return Err(ApiError::BadRequest(
                    "hard-budget media requires an explicit flat/unit price".into(),
                ));
            }
            let units = if self.components.iter().any(|c| c.kind == "unit") {
                media_units(endpoint, payload)?
            } else {
                0
            };
            return self
                .calculate(&Usage {
                    units,
                    ..Default::default()
                })
                .map(|v| v.0);
        }
        // Every chargeable token dimension may reach this conservative text ceiling.
        // Use the maximum configured tier rate, even when actual billing is marginal.
        let mut total = 0i128;
        for c in &self.components {
            let rate = c
                .tiers
                .iter()
                .map(|t| t.unit_price_micros)
                .fold(c.unit_price_micros, i64::max);
            let q = if c.kind == "flat" {
                1
            } else {
                i64::from(tokens)
            };
            let denominator = i128::from(c.unit_size) * 1_000_000;
            let numerator = i128::from(q) * i128::from(rate) * i128::from(self.ratio_millionths);
            total += (numerator + denominator - 1) / denominator;
        }
        i64::try_from(total).map_err(|_| invalid())
    }
}
/// Units are images, generated video seconds, or speech characters by protocol.
/// Uploaded audio has no trustworthy request-side duration and needs flat pricing.
pub fn media_units(endpoint: &str, payload: &Value) -> Result<i64, ApiError> {
    let units = match crate::providers::capability(endpoint) {
        "images" => payload.get("n").and_then(Value::as_i64).unwrap_or(1),
        "videos" => payload
            .get("seconds")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .ok_or_else(|| {
                ApiError::BadRequest("per-unit video pricing requires explicit seconds".into())
            })?,
        "speech" => payload["input"]
            .as_str()
            .map(|text| text.chars().count() as i64)
            .ok_or_else(invalid)?,
        _ => {
            return Err(ApiError::BadRequest(
                "hard-budget uploaded media requires flat pricing".into(),
            ));
        }
    };
    if units <= 0 || units > 1_000_000 {
        return Err(invalid());
    }
    Ok(units)
}
pub async fn snapshot(
    db: &DatabaseConnection,
    candidate: &Candidate,
    ratio: i64,
) -> Result<Price, ApiError> {
    let row=db.query_one(sql("SELECT id,schedule_json FROM model_prices WHERE model_id=? AND origin='operator' AND (provider_id=? OR provider_id IS NULL) AND valid_from<=? AND (valid_until IS NULL OR valid_until>?) ORDER BY provider_id IS NULL,version DESC LIMIT 1",vec![candidate.model_id.clone().into(),candidate.provider_id.clone().into(),db::now().into(),db::now().into()])).await?;
    let schedule = if let Some(row) = &row {
        serde_json::from_str::<super::schedule::Schedule>(
            &row.try_get::<String>("", "schedule_json")?,
        )
        .map_err(|_| invalid())?
    } else {
        Default::default()
    };
    let mut price_id = row
        .as_ref()
        .map(|r| r.try_get::<String>("", "id"))
        .transpose()?;
    let mut components = if let Some(id) = &price_id {
        let rows=db.query_all(sql("SELECT id,kind,unit_size,unit_price_micros,tiers_json FROM model_price_components WHERE price_id=? ORDER BY id",vec![id.clone().into()])).await?;
        let mut components = vec![];
        for row in rows {
            let document: Value = serde_json::from_str(&row.try_get::<String>("", "tiers_json")?)
                .map_err(|_| invalid())?;
            components.push(Component {
                id: row.try_get("", "id")?,
                kind: row.try_get("", "kind")?,
                unit_size: row.try_get("", "unit_size")?,
                unit_price_micros: row.try_get("", "unit_price_micros")?,
                tiers: serde_json::from_value(document.get("tiers").cloned().unwrap_or(json!([])))
                    .map_err(|_| invalid())?,
                cache_ttl: document
                    .get("cache_ttl")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
        components
    } else {
        vec![
            Component {
                id: String::new(),
                kind: "input".into(),
                unit_size: 1_000_000,
                unit_price_micros: candidate.target.input_price_micros,
                tiers: vec![],
                cache_ttl: None,
            },
            Component {
                id: String::new(),
                kind: "output".into(),
                unit_size: 1_000_000,
                unit_price_micros: candidate.target.output_price_micros,
                tiers: vec![],
                cache_ttl: None,
            },
        ]
    };
    if price_id.is_none() {
        let tx = db.begin().await?;
        tx.execute(sql(
            "UPDATE models SET priority=priority WHERE id=?",
            vec![candidate.model_id.clone().into()],
        ))
        .await?;
        let existing=tx.query_one(sql("SELECT p.id FROM model_prices p WHERE p.model_id=? AND p.origin='legacy_snapshot' AND EXISTS(SELECT 1 FROM model_price_components c WHERE c.price_id=p.id AND c.kind='input' AND c.unit_price_micros=?) AND EXISTS(SELECT 1 FROM model_price_components c WHERE c.price_id=p.id AND c.kind='output' AND c.unit_price_micros=?) ORDER BY p.version DESC LIMIT 1",vec![candidate.model_id.clone().into(),candidate.target.input_price_micros.into(),candidate.target.output_price_micros.into()])).await?;
        let snapshot = if let Some(row) = existing {
            let snapshot: String = row.try_get("", "id")?;
            for row in tx
                .query_all(sql(
                    "SELECT id,kind FROM model_price_components WHERE price_id=?",
                    vec![snapshot.clone().into()],
                ))
                .await?
            {
                let kind: String = row.try_get("", "kind")?;
                if let Some(component) = components.iter_mut().find(|c| c.kind == kind) {
                    component.id = row.try_get("", "id")?;
                }
            }
            snapshot
        } else {
            let snapshot = super::id();
            tx.execute(sql("INSERT INTO model_prices(id,model_id,version,valid_from,created_at,origin) SELECT ?,?,COALESCE(MAX(version),0)+1,?,?,'legacy_snapshot' FROM model_prices WHERE model_id=?",vec![snapshot.clone().into(),candidate.model_id.clone().into(),db::now().into(),db::now().into(),candidate.model_id.clone().into()])).await?;
            for component in &mut components {
                component.id = super::id();
                tx.execute(sql("INSERT INTO model_price_components(id,price_id,kind,unit_size,unit_price_micros) VALUES(?,?,?,?,?)",vec![component.id.clone().into(),snapshot.clone().into(),component.kind.clone().into(),component.unit_size.into(),component.unit_price_micros.into()])).await?;
            }
            snapshot
        };
        tx.commit().await?;
        price_id = Some(snapshot);
    }
    if let Some(overrides) = schedule.prices_at(db::now())? {
        for component in &mut components {
            if let Some(rate) = overrides.get(&component.kind) {
                component.unit_price_micros = *rate;
                component.tiers.clear();
            }
        }
    }
    let price = Price {
        id: price_id,
        model_id: candidate.model_id.clone(),
        ratio_millionths: ratio,
        components,
    };
    price.validate()?;
    Ok(price)
}
