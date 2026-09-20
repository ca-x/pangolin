use regex::Regex;
use sea_orm::{DatabaseConnection, FromQueryResult};
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

pub async fn load(db: &DatabaseConnection, project: &str) -> Result<Vec<Rule>> {
    #[derive(FromQueryResult)]
    struct Row {
        id: String,
        role_pattern: Option<String>,
        content_pattern: String,
        action: String,
        replacement: Option<String>,
        scopes_json: String,
        test_mode: bool,
    }
    Row::find_by_statement(statement("SELECT id,role_pattern,content_pattern,action,replacement,scopes_json,test_mode FROM prompt_protection_rules WHERE project_id=? AND enabled=1 ORDER BY created_at,id",vec![project.into()])).all(db).await?.into_iter().map(|r|{
        Ok(Rule{id:r.id,roles:r.role_pattern.as_deref().map(policy::regex).transpose()?,content:policy::regex(&r.content_pattern)?,
            deny:r.action=="deny",replacement:r.replacement.unwrap_or_else(||"[REDACTED]".into()),scopes:policy::document(&r.scopes_json)?,test:r.test_mode})
    }).collect()
}

pub async fn inject(
    db: &DatabaseConnection,
    project: &str,
    body: &mut Value,
    context: &Value,
    endpoint: &str,
) -> Result<()> {
    #[derive(FromQueryResult)]
    struct Prompt {
        role: String,
        content: String,
        activation_json: String,
    }
    let prompts=Prompt::find_by_statement(statement("SELECT role,content,activation_json FROM prompts WHERE project_id=? AND enabled=1 ORDER BY created_at,id",vec![project.into()])).all(db).await?;
    let mut prefix = vec![];
    for p in prompts {
        if policy::matches(&policy::document(&p.activation_json)?, context)? {
            prefix.push(json!({"role":p.role,"content":p.content}));
        }
    }
    if prefix.is_empty() {
        return Ok(());
    }
    if endpoint.starts_with("/v1/responses") {
        prefix.extend(super::session::input(body)?);
        body["input"] = Value::Array(prefix);
    } else if endpoint == "/v1/messages" {
        // Native Anthropic system prompts live outside messages.
        let mut system = vec![];
        prefix.retain(|p| {
            if p["role"] == "system" || p["role"] == "developer" {
                system.push(p["content"].as_str().unwrap_or("").to_owned());
                false
            } else {
                true
            }
        });
        if !system.is_empty() {
            let mut blocks = system
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
            body["system"] = Value::Array(blocks);
        }
        prefix.extend(
            body.get("messages")
                .and_then(Value::as_array)
                .cloned()
                .ok_or(Error::Invalid("messages must be an array"))?,
        );
        body["messages"] = Value::Array(prefix);
    } else {
        prefix.extend(
            body.get("messages")
                .and_then(Value::as_array)
                .cloned()
                .ok_or(Error::Invalid("messages must be an array"))?,
        );
        body["messages"] = Value::Array(prefix);
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
        for field in ["messages", "input"] {
            if let Some(value) = body.get_mut(field) {
                if let Value::Array(messages) = value {
                    for message in messages {
                        let role = message
                            .get("role")
                            .and_then(Value::as_str)
                            .unwrap_or("user")
                            .to_owned();
                        if let Some(content) = message.get_mut("content") {
                            protect_content(rule, &role, content, &mut hit)?;
                        }
                    }
                } else {
                    protect_content(rule, "user", value, &mut hit)?;
                }
            }
        }
        for field in ["system", "instructions"] {
            if let Some(content) = body.get_mut(field) {
                protect_content(rule, "system", content, &mut hit)?;
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
    if rule.roles.as_ref().is_some_and(|r| !r.is_match(role)) {
        return Ok(());
    }
    match content {
        Value::String(text) => {
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
                if let Some(text) = part.get_mut("text") {
                    protect_content(rule, role, text, hit)?;
                }
            }
        }
        _ => (),
    }
    Ok(())
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
    let permitted =
        |tool: &Value| tool_name(tool).is_some_and(|name| allowed.iter().any(|a| a == name));
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
