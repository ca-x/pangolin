use regex::Regex;
use sea_orm::{DatabaseConnection, FromQueryResult};
use serde::Serialize;
use serde_json::{Value, json};

use super::{Decision, Error, Result, policy, repository::statement};

pub struct Rule {
    id: String,
    roles: Option<Regex>,
    content: Regex,
    deny: bool,
    replacement: String,
    scopes: Value,
    test: bool,
    enabled: bool,
    state: String,
    allowlist: Vec<String>,
}

#[derive(FromQueryResult)]
struct RuleRow {
    id: String,
    name: String,
    description: String,
    role_pattern: Option<String>,
    content_pattern: String,
    action: String,
    replacement: Option<String>,
    scopes_json: String,
    test_mode: bool,
    enabled: bool,
    state: String,
    allowlist_json: String,
}

impl RuleRow {
    fn compile(&self) -> Result<Rule> {
        Ok(Rule {
            id: self.id.clone(),
            roles: self
                .role_pattern
                .as_deref()
                .map(policy::regex)
                .transpose()?,
            content: policy::regex(&self.content_pattern)?,
            deny: self.action == "deny",
            replacement: self
                .replacement
                .clone()
                .unwrap_or_else(|| "[REDACTED]".into()),
            scopes: policy::document(&self.scopes_json)?,
            test: self.test_mode,
            enabled: self.enabled,
            state: self.state.clone(),
            allowlist: validate_allowlist(
                &serde_json::from_str::<Value>(&self.allowlist_json)
                    .map_err(|_| Error::Configuration)?,
            )?,
        })
    }
}

#[derive(Serialize)]
pub struct Preview {
    id: String,
    name: String,
    description: String,
    action: String,
    enabled: bool,
    state: String,
    matched: bool,
    result: String,
}

const PREVIEW_RULE_LIMIT: usize = 500;

pub fn validate_allowlist(value: &Value) -> Result<Vec<String>> {
    let entries = value
        .as_array()
        .filter(|entries| entries.len() <= 64)
        .ok_or(Error::Invalid(
            "protection allowlist must contain at most 64 strings of 256 bytes",
        ))?;
    entries
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .filter(|text| text.len() <= 256)
                .map(str::to_owned)
                .ok_or(Error::Invalid(
                    "protection allowlist must contain at most 64 strings of 256 bytes",
                ))
        })
        .collect()
}

pub async fn load(db: &DatabaseConnection, project: &str) -> Result<Vec<Rule>> {
    load_rules(db, project, false).await
}
async fn load_rules(db: &DatabaseConnection, project: &str, all: bool) -> Result<Vec<Rule>> {
    let query = if all {
        "SELECT id,name,description,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,state,allowlist_json FROM prompt_protection_rules WHERE project_id=? ORDER BY created_at,id LIMIT 501"
    } else {
        "SELECT id,name,description,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,state,allowlist_json FROM prompt_protection_rules WHERE project_id=? AND enabled=1 AND state='active' ORDER BY created_at,id LIMIT 501"
    };
    let rows = RuleRow::find_by_statement(statement(query, vec![project.into()]))
        .all(db)
        .await?;
    if rows.len() > PREVIEW_RULE_LIMIT {
        return Err(Error::Invalid(
            "protection inspection supports at most 500 rules",
        ));
    }
    rows.into_iter().map(|row| row.compile()).collect()
}

/// Retain the legacy independent text preview for each rule without mutating
/// request state. Deny rules leave the sample unchanged. Full-request preview
/// additionally evaluates roles, scopes, allowlists and ordered transformations
/// through the same inspection engine as online enforcement.
pub async fn preview(db: &DatabaseConnection, project: &str, text: &str) -> Result<Vec<Preview>> {
    let rows = RuleRow::find_by_statement(statement(
        "SELECT id,name,description,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,state,allowlist_json FROM prompt_protection_rules WHERE project_id=? ORDER BY created_at,id LIMIT 501",
        vec![project.into()],
    ))
    .all(db)
    .await?;
    if rows.len() > PREVIEW_RULE_LIMIT {
        return Err(Error::Invalid(
            "protection preview supports at most 500 rules",
        ));
    }
    rows.into_iter()
        .map(|row| {
            let rule = row.compile()?;
            let matched = rule.content.is_match(text);
            let result = if matched && !rule.deny {
                rule.content
                    .replace_all(text, regex::NoExpand(&rule.replacement))
                    .into_owned()
            } else {
                text.to_owned()
            };
            Ok(Preview {
                id: row.id,
                name: row.name,
                description: row.description,
                action: row.action,
                enabled: row.enabled,
                state: row.state,
                matched,
                result,
            })
        })
        .collect()
}

pub async fn inject(
    db: &DatabaseConnection,
    project: &str,
    body: &mut Value,
    context: &Value,
    endpoint: &str,
    decisions: &mut Vec<super::Decision>,
) -> Result<()> {
    #[derive(FromQueryResult)]
    struct Prompt {
        role: String,
        content: String,
        activation_json: String,
        action: String,
    }
    let prompts=Prompt::find_by_statement(statement("SELECT role,content,activation_json,action FROM prompts WHERE project_id=? AND enabled=1 ORDER BY \"order\",created_at,id",vec![project.into()])).all(db).await?;
    let mut prefix = vec![];
    let mut suffix = vec![];
    for p in prompts {
        if policy::matches(&policy::document(&p.activation_json)?, context)? {
            let prompt = json!({"role":p.role,"content":p.content});
            match p.action.as_str() {
                "prepend" => prefix.push(prompt),
                "append" => suffix.push(prompt),
                _ => return Err(Error::Configuration),
            }
        }
    }
    if prefix.is_empty() && suffix.is_empty() {
        return Ok(());
    }
    if endpoint.starts_with("/v1/responses") {
        prefix.extend(super::session::input(body)?);
        prefix.append(&mut suffix);
        body["input"] = Value::Array(prefix);
    } else if endpoint == "/v1/messages" {
        // Native Anthropic system prompts live outside messages.
        let mut system_before = vec![];
        prefix.retain(|p| {
            if p["role"] == "system" || p["role"] == "developer" {
                system_before.push(p["content"].as_str().unwrap_or("").to_owned());
                false
            } else {
                true
            }
        });
        let mut system_after = vec![];
        suffix.retain(|p| {
            if p["role"] == "system" || p["role"] == "developer" {
                system_after.push(p["content"].as_str().unwrap_or("").to_owned());
                false
            } else {
                true
            }
        });
        if !system_before.is_empty() || !system_after.is_empty() {
            let mut blocks = system_before
                .into_iter()
                .map(|s| json!({"type":"text","text":s}))
                .collect::<Vec<_>>();
            if let Some(existing) = body.get("system") {
                match existing {
                    Value::String(text) => blocks.push(json!({"type":"text","text":text})),
                    Value::Array(items) => blocks.extend(items.clone()),
                    _ => return Err(Error::Invalid("invalid system prompt")),
                }
            }
            blocks.extend(
                system_after
                    .into_iter()
                    .map(|s| json!({"type":"text","text":s})),
            );
            body["system"] = Value::Array(blocks);
        }
        prefix.extend(
            body.get("messages")
                .and_then(Value::as_array)
                .cloned()
                .ok_or(Error::Invalid("messages must be an array"))?,
        );
        prefix.append(&mut suffix);
        body["messages"] = Value::Array(prefix);
    } else if endpoint.starts_with("/v1beta/models:") {
        let mut contents = body
            .get("contents")
            .and_then(Value::as_array)
            .cloned()
            .ok_or(Error::Invalid("contents must be an array"))?;
        let mut instructions = body
            .get("systemInstruction")
            .and_then(|v| v.get("parts"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut content_before = vec![];
        let mut instruction_before = vec![];
        for prompt in prefix {
            if prompt["role"] == "system" || prompt["role"] == "developer" {
                instruction_before.push(json!({"text":prompt["content"]}));
            } else {
                content_before.push(json!({"role":if prompt["role"]=="assistant"{"model"}else{"user"},"parts":[{"text":prompt["content"]}]}));
            }
        }
        let mut content_after = vec![];
        let mut instruction_after = vec![];
        for prompt in suffix {
            if prompt["role"] == "system" || prompt["role"] == "developer" {
                instruction_after.push(json!({"text":prompt["content"]}));
            } else {
                content_after.push(json!({"role":if prompt["role"]=="assistant"{"model"}else{"user"},"parts":[{"text":prompt["content"]}]}));
            }
        }
        content_before.append(&mut contents);
        content_before.append(&mut content_after);
        // Non-system prompts keep Gemini's native role/parts shape around the
        // existing contents. System instructions use their native container.
        body["contents"] = Value::Array(content_before);
        instruction_before.append(&mut instructions);
        instruction_before.append(&mut instruction_after);
        if !instruction_before.is_empty() {
            body["systemInstruction"] = json!({"parts":instruction_before});
        }
    } else if endpoint == "/v1/chat/completions" {
        prefix.extend(
            body.get("messages")
                .and_then(Value::as_array)
                .cloned()
                .ok_or(Error::Invalid("messages must be an array"))?,
        );
        prefix.append(&mut suffix);
        body["messages"] = Value::Array(prefix);
    } else {
        // This endpoint cannot carry injected messages. Skipping is what the
        // reference product does; failing here meant one prompt with the default
        // activation (which matches everything) answered 400 for the whole
        // project on embeddings and every other non-conversational route. The
        // decision is recorded so the skip is visible in the trace rather than
        // silent.
        decisions.push(super::Decision {
            stage: "prompt",
            candidate: None,
            reason: "prompt_injection_skipped_unsupported_endpoint",
        });
        return Ok(());
    }
    Ok(())
}

#[derive(Serialize)]
pub struct Finding {
    rule_id: String,
    path: String,
    start: usize,
    end: usize,
    action: &'static str,
    reason: &'static str,
}
#[derive(Serialize)]
pub struct RequestPreview {
    decision: &'static str,
    redacted_body: Value,
    findings: Vec<Finding>,
    suppressed_findings: Vec<Finding>,
    truncated: bool,
}

pub fn templates() -> Value {
    json!({"templates":[
        {"template_id":"email","name":"Email","description":"Email-like text; review false positives before enabling","content_pattern":r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,63}\b","action":"redact","replacement":"[REDACTED]","allowlist":[],"enabled":false},
        {"template_id":"phone","name":"Phone","description":"International phone-like text; review false positives before enabling","content_pattern":r"\+[1-9][0-9 ()-]{8,20}[0-9]","action":"redact","replacement":"[REDACTED]","allowlist":[],"enabled":false},
        {"template_id":"api_key","name":"API key-like","description":"Common API key-like shapes; detection is not a credential guarantee","content_pattern":r"\b(?:sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{20,})\b","action":"redact","replacement":"[REDACTED]","allowlist":[],"enabled":false}
    ]})
}

pub async fn request_preview(
    db: &DatabaseConnection,
    project: &str,
    endpoint: &str,
    body: Value,
) -> Result<RequestPreview> {
    if !body.is_object() || body.to_string().len() > 64 * 1024 {
        return Err(Error::Invalid(
            "protection request preview requires an object up to 64 KiB",
        ));
    }
    super::inspection::validate_shape(&body, 0)?;
    let context = policy::context(
        &body,
        &http::HeaderMap::new(),
        endpoint,
        Some((project, "")),
    );
    let rules = load_rules(db, project, true).await?;
    inspect(&rules, body, &context, &mut vec![])
}

pub fn apply(
    rules: &[Rule],
    body: &mut Value,
    context: &Value,
    decisions: &mut Vec<Decision>,
) -> Result<()> {
    if rules.is_empty() {
        return Ok(());
    }
    let outcome = inspect(rules, body.clone(), context, decisions)?;
    if outcome.decision == "deny" {
        return Err(Error::Forbidden);
    }
    *body = outcome.redacted_body;
    Ok(())
}

fn inspect(
    rules: &[Rule],
    mut body: Value,
    context: &Value,
    decisions: &mut Vec<Decision>,
) -> Result<RequestPreview> {
    let endpoint = context["endpoint"].as_str().unwrap_or("");
    // Check the whole relevant shape before a match can stop on an earlier deny.
    super::inspection::visit(&mut body, endpoint, &mut |_, _, _| Ok(()))?;
    let mut result = RequestPreview {
        decision: "allow",
        redacted_body: Value::Null,
        findings: vec![],
        suppressed_findings: vec![],
        truncated: false,
    };
    for rule in rules {
        let suppression = if !rule.enabled {
            Some("disabled")
        } else if rule.state != "active" {
            Some("inactive")
        } else if !policy::matches(&rule.scopes, context)? {
            Some("scope_mismatch")
        } else {
            None
        };
        let mut hit = false;
        let mut denied = false;
        let mut matched = false;
        super::inspection::visit(&mut body, endpoint, &mut |role, path, text| {
            let mut output = String::new();
            let mut previous = 0;
            let mut text_matched = false;
            for span in rule.content.find_iter(text) {
                if result.findings.len() + result.suppressed_findings.len() >= 512 {
                    return Err(Error::Invalid("privacy inspection findings exceed 512"));
                }
                matched = true;
                text_matched = true;
                let reason = suppression
                    .or_else(|| {
                        rule.roles
                            .as_ref()
                            .filter(|pattern| !pattern.is_match(role))
                            .map(|_| "role_mismatch")
                    })
                    .or_else(|| {
                        rule.allowlist
                            .iter()
                            .any(|allowed| allowed == span.as_str())
                            .then_some("allowlisted")
                    })
                    .or_else(|| rule.test.then_some("test_mode"));
                let finding = Finding {
                    rule_id: rule.id.clone(),
                    path: path.to_owned(),
                    start: span.start(),
                    end: span.end(),
                    action: if rule.deny { "deny" } else { "redact" },
                    reason: reason.unwrap_or("matched"),
                };
                output.push_str(&text[previous..span.start()]);
                if let Some(reason) = reason {
                    if reason == "test_mode" {
                        hit = true;
                    }
                    output.push_str(span.as_str());
                    result.suppressed_findings.push(finding);
                } else {
                    hit = true;
                    if rule.deny {
                        denied = true;
                        output.push_str(span.as_str());
                    } else {
                        output.push_str(&rule.replacement);
                        result.decision = "redact";
                    }
                    result.findings.push(finding);
                }
                previous = span.end();
            }
            if text_matched {
                output.push_str(&text[previous..]);
                *text = output;
            }
            Ok(())
        })?;
        if !matched {
            if result.findings.len() + result.suppressed_findings.len() >= 512 {
                return Err(Error::Invalid("privacy inspection findings exceed 512"));
            }
            result.suppressed_findings.push(Finding {
                rule_id: rule.id.clone(),
                path: String::new(),
                start: 0,
                end: 0,
                action: if rule.deny { "deny" } else { "redact" },
                reason: suppression.unwrap_or("no_match"),
            });
        }
        if hit {
            decisions.push(Decision {
                stage: "protection",
                candidate: Some(rule.id.clone()),
                reason: if rule.test {
                    "test_match"
                } else if denied {
                    "denied"
                } else {
                    "redacted"
                },
            });
        }
        if denied {
            result.decision = "deny";
            break;
        }
    }
    result.redacted_body = body;
    Ok(result)
}

fn tool_name(tool: &Value) -> Option<&str> {
    tool.get("name")
        .or_else(|| tool.get("function").and_then(|f| f.get("name")))
        .and_then(Value::as_str)
}

pub fn tools(body: &mut Value, allowed: Option<&[String]>) -> Result<()> {
    let Some(allowed) = allowed else {
        return Ok(());
    };
    // Legacy fields are a separate upstream execution surface. Reject them when
    // a tool policy is active, including fields added by channel overrides.
    if body.get("functions").is_some() || body.get("function_call").is_some() {
        return Err(Error::Invalid(
            "use tools and tool_choice when an allowed-tools policy is active",
        ));
    }
    let permitted =
        |tool: &Value| tool_name(tool).is_some_and(|name| allowed.iter().any(|a| a == name));
    if body.get("contents").is_some() {
        if let Some(names) = body
            .pointer("/toolConfig/functionCallingConfig/allowedFunctionNames")
            .and_then(Value::as_array)
            && names.iter().any(|name| {
                name.as_str()
                    .is_none_or(|name| !allowed.iter().any(|a| a == name))
            })
        {
            return Err(Error::Forbidden);
        }
        if let Some(tools) = body.get_mut("tools") {
            for tool in tools
                .as_array_mut()
                .ok_or(Error::Invalid("tools must be an array"))?
            {
                if tool.as_object().is_none_or(|object| {
                    object.len() != 1 || !object.contains_key("functionDeclarations")
                }) {
                    return Err(Error::Invalid(
                        "native Gemini built-in tools are unsupported under an allowed-tools policy",
                    ));
                }
                let declarations = tool
                    .get_mut("functionDeclarations")
                    .and_then(Value::as_array_mut)
                    .ok_or(Error::Invalid("functionDeclarations must be an array"))?;
                declarations.retain(permitted);
            }
        }
        return Ok(());
    }
    if let Some(choice) = body.get("tool_choice") {
        if tool_name(choice).is_some() && !permitted(choice) {
            return Err(Error::Forbidden);
        }
        if choice["type"] == "allowed_tools"
            && choice
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| tools.iter().any(|tool| !permitted(tool)))
        {
            return Err(Error::Forbidden);
        }
    }
    if let Some(tools) = body.get_mut("tools") {
        let tools = tools
            .as_array_mut()
            .ok_or(Error::Invalid("tools must be an array"))?;
        tools.retain(permitted);
        if tools.is_empty() && body.get("tool_choice").and_then(Value::as_str) == Some("required") {
            return Err(Error::Forbidden);
        }
    }
    // Historical tool calls/results are intentionally left in their original order.
    Ok(())
}

pub fn transform(body: &mut Value, rules: &Value) -> Result<()> {
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if rules.get("developer_to_system").and_then(Value::as_bool) == Some(true)
                && message["role"] == "developer"
            {
                message["role"] = json!("system");
            }
            if rules.get("force_content_array").and_then(Value::as_bool) == Some(true)
                && let Some(text) = message.get("content").and_then(Value::as_str)
            {
                message["content"] = json!([{"type":"text","text":text}]);
            }
        }
    }
    if let Some(mapping) = rules.get("reasoning_effort") {
        let effort = body
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        if let Some(mapped) = mapping.get(effort) {
            if !mapped.is_string() {
                return Err(Error::Configuration);
            }
            body["reasoning_effort"] = mapped.clone();
        }
    }
    Ok(())
}
