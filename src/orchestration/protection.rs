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

pub async fn load(db: &DatabaseConnection, project: &str) -> Result<Vec<Rule>> {
    RuleRow::find_by_statement(statement(
        "SELECT id,name,description,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,state FROM prompt_protection_rules WHERE project_id=? AND enabled=1 AND state='active' ORDER BY created_at,id",
        vec![project.into()],
    ))
    .all(db)
    .await?
    .into_iter()
    .map(|row| row.compile())
    .collect()
}

/// Evaluate every rule in a project against an operator-provided sample without
/// mutating request state. The same compiled content matcher and replacement
/// semantics are used by live enforcement; deny rules report the unchanged text
/// because their effect is refusal, not redaction.
pub async fn preview(db: &DatabaseConnection, project: &str, text: &str) -> Result<Vec<Preview>> {
    let rows = RuleRow::find_by_statement(statement(
        "SELECT id,name,description,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,state FROM prompt_protection_rules WHERE project_id=? ORDER BY created_at,id LIMIT 501",
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

pub fn apply(
    rules: &[Rule],
    body: &mut Value,
    context: &Value,
    decisions: &mut Vec<Decision>,
) -> Result<()> {
    for rule in rules {
        if !policy::matches(&rule.scopes, context)? {
            continue;
        }
        let mut hit = false;
        for field in ["messages", "input", "contents"] {
            if let Some(value) = body.get_mut(field) {
                if let Value::Array(messages) = value {
                    for message in messages {
                        if message.is_string() {
                            protect_content(rule, "user", message, &mut hit)?;
                            continue;
                        }
                        if message.get("text").is_some()
                            || matches!(
                                message.get("type").and_then(Value::as_str),
                                Some("text" | "input_text")
                            )
                        {
                            protect_content(rule, "user", message, &mut hit)?;
                            continue;
                        }
                        let is_tool_output = matches!(
                            message.get("type").and_then(Value::as_str),
                            Some("function_call_output" | "custom_tool_call_output")
                        );
                        let role = if is_tool_output {
                            "tool"
                        } else {
                            message
                                .get("role")
                                .and_then(Value::as_str)
                                .unwrap_or("user")
                        }
                        .to_owned();
                        if let Some(content) = message.get_mut("content") {
                            protect_content(rule, &role, content, &mut hit)?;
                        }
                        if let Some(parts) = message.get_mut("parts") {
                            protect_content(rule, &role, parts, &mut hit)?;
                        }
                        if is_tool_output && let Some(output) = message.get_mut("output") {
                            protect_content(rule, "tool", output, &mut hit)?;
                        }
                    }
                } else {
                    protect_content(rule, "user", value, &mut hit)?;
                }
            }
        }
        for field in ["system", "instructions", "systemInstruction"] {
            if let Some(content) = body.get_mut(field) {
                protect_content(rule, "system", content, &mut hit)?;
            }
        }
        for field in ["prompt", "query", "documents", "content"] {
            if let Some(content) = body.get_mut(field) {
                protect_content(rule, "user", content, &mut hit)?;
            }
        }
        if hit {
            decisions.push(Decision {
                stage: "protection",
                candidate: Some(rule.id.clone()),
                reason: if rule.test { "test_match" } else { "redacted" },
            });
        }
    }
    Ok(())
}

fn protect_content(rule: &Rule, role: &str, content: &mut Value, hit: &mut bool) -> Result<()> {
    match content {
        Value::String(text) => {
            if rule.roles.as_ref().is_some_and(|r| !r.is_match(role)) {
                return Ok(());
            }
            if rule.content.is_match(text) {
                *hit = true;
                if !rule.test {
                    if rule.deny {
                        return Err(Error::Forbidden);
                    }
                    *text = rule
                        .content
                        .replace_all(text, regex::NoExpand(&rule.replacement))
                        .into_owned();
                }
            }
        }
        Value::Array(parts) => {
            for part in parts {
                protect_content(rule, role, part, hit)?;
            }
        }
        Value::Object(part) => {
            // Anthropic represents tool output inside a user message. Role
            // filtering applies to the semantic tool output, not its wrapper.
            let role = if part.get("type").and_then(Value::as_str) == Some("tool_result") {
                "tool"
            } else {
                role
            };
            if let Some(text) = part.get_mut("text") {
                protect_content(rule, role, text, hit)?;
            }
            if let Some(nested) = part.get_mut("content") {
                protect_content(rule, role, nested, hit)?;
            }
            if let Some(parts) = part.get_mut("parts") {
                protect_content(rule, role, parts, hit)?;
            }
            if let Some(response) = part
                .get_mut("functionResponse")
                .and_then(|v| v.get_mut("response"))
            {
                protect_tool_json(rule, response, hit)?;
            }
            if let Some(response) = part
                .get_mut("function_response")
                .and_then(|value| value.get_mut("response"))
            {
                protect_tool_json(rule, response, hit)?;
            }
        }
        _ => (),
    }
    Ok(())
}

fn protect_tool_json(rule: &Rule, value: &mut Value, hit: &mut bool) -> Result<()> {
    match value {
        Value::String(_) => protect_content(rule, "tool", value, hit),
        Value::Array(items) => {
            for item in items {
                protect_tool_json(rule, item, hit)?;
            }
            Ok(())
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                protect_tool_json(rule, value, hit)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
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
