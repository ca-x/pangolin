//! The first user input of a stored request body, for the trace list.
//!
//! A trace row is what an operator triages from, and "what did the caller actually
//! ask" is the fact that makes it readable. The answer is already stored — the
//! logging policy wrote the sanitized request body to
//! `request_contents.request_json` at admission — so this module only *reads* what
//! is there, and never reconstructs it:
//!
//! - only text a user sent: never a system prompt, an assistant turn, a tool call
//!   or its result, a media reference, or a nested object rendered as JSON;
//! - only the shapes Pangolin stores: OpenAI/Responses `messages` and `input`,
//!   Anthropic `messages` with `content` blocks, Gemini `contents` with `parts`;
//! - nothing at all when the policy stored no body (`off`/`metadata`), when the
//!   stored JSON cannot be read, or when the body holds no user text.
//!
//! Attribution fails closed, per field, because the three fields do not share a
//! contract ([`Shape`]): a `messages` turn is the user's only when it says
//! `role: "user"`, a Responses item is read only for the types this build knows
//! (and a `message` there names its role too), and a Gemini turn may leave its role
//! out — which is the user's — but never a role that is somebody else's. An item or
//! block whose type this build does not recognise is not read at all, so the next
//! protocol revision cannot hand an assistant turn to the console as the user's
//! words.
//!
//! The result is whitespace-normalized and bounded, so a page of traces cannot
//! carry a megabyte of prompt per row. Extraction lives here, in Rust, rather than
//! in a multi-protocol `json_extract` expression: the protocol shapes are the
//! gateway's own contract and are tested as such.

use serde_json::Value;

/// The longest preview the list may carry, counted in Unicode characters *and* in
/// bytes. Both bounds apply, so the smaller one wins for a given text: 240 CJK
/// characters are 720 bytes, and 240 bytes are 60 of them.
const PREVIEW_LIMIT: usize = 240;

/// Which conversation field a body's turns came from. The rules are not
/// interchangeable, so the field is passed down rather than guessed at the item.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// OpenAI chat and Anthropic messages: every item is a message, and a message
    /// is the user's only when it says so.
    Messages,
    /// The Responses API's `input`: the whole field may be a string, and its items
    /// are the typed ones plus the untyped shape that predates them.
    Input,
    /// Gemini `contents`: a turn may leave its role out, which is the user's.
    Contents,
}

/// The conversation fields a protocol puts its turns in, in the order a body that
/// carries several is read.
const SHAPES: [(&str, Shape); 3] = [
    ("messages", Shape::Messages),
    ("input", Shape::Input),
    ("contents", Shape::Contents),
];

/// The first textual user input of a stored request body, or `None` when there is
/// no stored body, nothing in it a user wrote, or nothing that can be read.
pub(super) fn first_user_query(stored: Option<&str>) -> Option<String> {
    let body: Value = serde_json::from_str(stored?).ok()?;
    let text = SHAPES
        .iter()
        .find_map(|(field, shape)| body.get(field).and_then(|value| shape.text(value)))?;
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then(|| bound(&normalized))
}

impl Shape {
    /// The first user text inside this field.
    fn text(self, value: &Value) -> Option<String> {
        match value {
            // Only the Responses API accepts a bare string as a whole field.
            Value::String(text) => (self == Self::Input).then(|| text.clone()),
            Value::Array(items) => items.iter().find_map(|item| self.item(item)),
            _ => None,
        }
    }

    /// One turn of this field, if it is a user turn and holds text.
    fn item(self, item: &Value) -> Option<String> {
        let message = match item {
            // A bare string item is the Responses API's; in `messages` or `contents`
            // it is not a turn at all, and a turn nobody can attribute is not read.
            Value::String(text) => return (self == Self::Input).then(|| text.clone()),
            Value::Object(object) => object,
            _ => return None,
        };
        let kind = message.get("type").and_then(Value::as_str);
        // A tool call or its result is never the user's own words, whatever wraps it.
        if kind.is_some_and(is_tool_shape) {
            return None;
        }
        if !self.is_user_turn(kind, message.get("role").and_then(Value::as_str)) {
            return None;
        }
        for field in ["content", "parts"] {
            if let Some(value) = message.get(field)
                && let Some(text) = text_in(value)
            {
                return Some(text);
            }
        }
        // A bare text item, `{"type":"input_text","text":"…"}`.
        message
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    /// Whether this turn belongs to the user under *this field's* contract.
    fn is_user_turn(self, kind: Option<&str>, role: Option<&str>) -> bool {
        match self {
            // A chat message that does not name its role cannot be attributed to
            // anyone, and every role that is not `user` belongs to somebody else.
            Self::Messages => role == Some("user"),
            // Gemini may leave the first turn's role out, which is the user's; a
            // named role that is not `user` is the model's or the system's.
            Self::Contents => matches!(role, None | Some("user")),
            // Responses items: a `message` names its role like any other message, a
            // bare text item carries the user's text, and an untyped item is the
            // shape that predates the typed ones. Anything else typed — an
            // `assistant_message` today, whatever the next revision adds tomorrow —
            // is refused rather than assumed to be the user's.
            Self::Input => match kind {
                Some("message") => role == Some("user"),
                Some("input_text") => matches!(role, None | Some("user")),
                Some(_) => false,
                None => matches!(role, None | Some("user")),
            },
        }
    }
}

/// Text inside a message body: a bare string, or the first text block of a
/// protocol's part list.
fn text_in(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => parts.iter().find_map(text_part),
        // An object is a shape this preview does not know, and flattening it to
        // JSON would publish a structure instead of a sentence.
        _ => None,
    }
}

/// One content block or Gemini part, if it carries text rather than media, a tool
/// result or a nested structure.
fn text_part(part: &Value) -> Option<String> {
    match part {
        Value::String(text) => Some(text.clone()),
        Value::Object(object) => {
            let kind = object.get("type").and_then(Value::as_str);
            // A typed block is read only when its type is a text one: Anthropic's
            // `text` and the Responses API's `input_text`. `output_text` is an
            // assistant's, a tool block is a tool's, and a type this build does not
            // know is not read at all. Gemini's parts carry no `type`.
            if kind.is_some_and(|kind| !matches!(kind, "text" | "input_text")) {
                return None;
            }
            // Anthropic and Gemini wrap a tool result or a media payload in a part
            // of its own. It is not the user's words even when it holds text.
            if [
                "functionResponse",
                "functionCall",
                "inlineData",
                "fileData",
                "executableCode",
                "codeExecutionResult",
            ]
            .iter()
            .any(|key| object.contains_key(*key))
            {
                return None;
            }
            object
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }
        _ => None,
    }
}

/// The item and block types that carry a tool's payload rather than a user's words.
/// They are named here as a second refusal, beside the type allow-lists above: a
/// tool block must not be read even from a field that accepts untyped items.
fn is_tool_shape(kind: &str) -> bool {
    matches!(
        kind,
        "function_call"
            | "function_call_output"
            | "custom_tool_call"
            | "custom_tool_call_output"
            | "tool_call"
            | "tool_call_output"
            | "tool_result"
            | "tool_use"
            | "reasoning"
            | "computer_call"
            | "computer_call_output"
            | "mcp_call"
            | "mcp_approval_request"
            | "mcp_approval_response"
            | "file_search_call"
            | "web_search_call"
            | "code_interpreter_call"
            | "image_generation_call"
            | "local_shell_call"
    )
}

/// The preview, cut to the bound without splitting a character.
fn bound(text: &str) -> String {
    let mut out = String::new();
    for (characters, character) in text.chars().enumerate() {
        if characters >= PREVIEW_LIMIT || out.len() + character.len_utf8() > PREVIEW_LIMIT {
            break;
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preview(body: Value) -> Option<String> {
        first_user_query(Some(&body.to_string()))
    }

    /// Every shape the gateway accepts and stores, read back as the caller wrote it.
    #[test]
    fn a_trace_preview_reads_the_first_user_text_of_every_stored_shape() {
        // OpenAI chat: content is a string.
        assert_eq!(
            preview(
                json!({"model":"public","messages":[{"role":"system","content":"be terse"},{"role":"user","content":"first question"},{"role":"user","content":"second question"}]})
            ),
            Some("first question".into())
        );
        // OpenAI chat: content is a part list, including the newer part name.
        assert_eq!(
            preview(
                json!({"messages":[{"role":"user","content":[{"type":"text","text":"from parts"}]}]})
            ),
            Some("from parts".into())
        );
        assert_eq!(
            preview(
                json!({"messages":[{"role":"user","content":[{"type":"input_text","text":"typed part"}]}]})
            ),
            Some("typed part".into())
        );
        // Responses: the whole input is a string, or a list of items.
        assert_eq!(
            preview(json!({"input":"bare input"})),
            Some("bare input".into())
        );
        assert_eq!(
            preview(
                json!({"instructions":"system text","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"responses question"}]}]})
            ),
            Some("responses question".into())
        );
        assert_eq!(
            preview(json!({"input":[{"type":"input_text","text":"bare text item"}]})),
            Some("bare text item".into())
        );
        // Anthropic: user turns carry content blocks, and a text block may follow a
        // tool result inside the same turn.
        assert_eq!(
            preview(
                json!({"system":"system text","messages":[{"role":"user","content":[{"type":"text","text":"anthropic question"}]}]})
            ),
            Some("anthropic question".into())
        );
        assert_eq!(
            preview(
                json!({"messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"tool output"},{"type":"text","text":"after the tool"}]}]})
            ),
            Some("after the tool".into())
        );
        // Gemini: contents hold parts, and the role of the first turn may be absent.
        assert_eq!(
            preview(
                json!({"systemInstruction":{"parts":[{"text":"system text"}]},"contents":[{"role":"user","parts":[{"text":"gemini question"}]}]})
            ),
            Some("gemini question".into())
        );
        assert_eq!(
            preview(json!({"contents":[{"parts":[{"text":"no role at all"}]}]})),
            Some("no role at all".into())
        );
    }

    /// A turn is the user's only where the field's own contract says so.
    ///
    /// The preview must not be widened by the next protocol revision. An item whose
    /// `type` this build does not know is not a user turn, and neither is a message
    /// that does not name its role: `assistant_message` today, an assistant turn of
    /// some later shape tomorrow, and the list would publish it as the user's words.
    #[test]
    fn a_trace_preview_reads_a_user_turn_only_where_the_field_says_so() {
        let refused = [
            // `messages` is a list of *messages*: the role is part of the shape, so
            // an item without one is not attributable to the user — and a bare
            // string is not a message at all.
            json!({"messages":[{"text":"no role at all"}]}),
            json!({"messages":[{"content":"no role at all"}]}),
            json!({"messages":["a bare string is not a message"]}),
            // Responses `input`: a typed item is read only for the types this build
            // knows, and a `message` names its role like any other message.
            json!({"input":[{"type":"assistant_message","text":"assistant text"}]}),
            json!({"input":[{"type":"assistant_message","role":"assistant","content":"assistant text"}]}),
            json!({"input":[{"type":"system_message","text":"system text"}]}),
            json!({"input":[{"type":"system_message","role":"system","content":"system text"}]}),
            json!({"input":[{"type":"something_new","text":"text of an item this build does not know"}]}),
            json!({"input":[{"type":"something_new","content":"content of an item this build does not know"}]}),
            json!({"input":[{"type":"message","content":"a message without a role"}]}),
            json!({"input":[{"type":"message","text":"a message without a role"}]}),
            json!({"input":[{"type":"input_text","role":"assistant","text":"not the user's"}]}),
            // Gemini `contents` may leave the role out, which is the user's; a role
            // that is not `user` is somebody else's, whoever adds it.
            json!({"contents":[{"role":"model","parts":[{"text":"assistant text"}]}]}),
            json!({"contents":[{"role":"system","parts":[{"text":"system text"}]}]}),
            json!({"contents":[{"role":"function","parts":[{"text":"tool text"}]}]}),
            // And a part inside an otherwise valid user turn is read only when its
            // own type is a text one: `output_text` is an assistant's.
            json!({"messages":[{"role":"user","content":[{"type":"output_text","text":"assistant text"}]}]}),
            json!({"input":[{"type":"message","role":"user","content":[{"type":"output_text","text":"assistant text"}]}]}),
        ];
        for body in refused {
            assert_eq!(preview(body.clone()), None, "{body} must not be previewed");
        }
        // Every shape that *is* the user's stays readable: a chat message with the
        // role, a Responses string and its untyped and typed items, and a Gemini
        // turn without a role.
        let read = [
            json!({"messages":[{"role":"user","content":"chat message"}]}),
            json!({"input":"the whole input as a string"}),
            json!({"input":["a bare item"]}),
            json!({"input":[{"content":"an untyped item"}]}),
            json!({"input":[{"role":"user","content":"an untyped item with a role"}]}),
            json!({"input":[{"type":"input_text","text":"a bare text item"}]}),
            json!({"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"a typed message"}]}]}),
            json!({"contents":[{"parts":[{"text":"a turn with no role"}]}]}),
            json!({"contents":[{"role":"user","parts":[{"text":"a turn with the role"}]}]}),
            json!({"messages":[{"role":"user","content":[{"type":"text","text":"a typed text block"}]}]}),
        ];
        for body in read {
            assert!(
                preview(body.clone()).is_some(),
                "{body} is the user's and must be previewed"
            );
        }
    }

    /// Nothing a user did not write is ever previewed.
    #[test]
    fn a_trace_preview_never_reads_system_assistant_or_tool_text() {
        let excluded = [
            json!({"messages":[{"role":"system","content":"system text"}]}),
            json!({"messages":[{"role":"assistant","content":"assistant text"}]}),
            json!({"messages":[{"role":"developer","content":"developer text"}]}),
            json!({"messages":[{"role":"tool","content":"tool output"}]}),
            json!({"messages":[{"role":"user","content":[{"type":"tool_result","content":"tool output"}]}]}),
            json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.test/a.png"}}]}]}),
            json!({"messages":[{"role":"user","content":[{"type":"image","source":{"data":"aGk="}}]}]}),
            json!({"input":[{"type":"function_call_output","call_id":"t1","output":"tool output"}]}),
            json!({"input":[{"type":"custom_tool_call","call_id":"t1","input":"tool input"}]}),
            json!({"input":[{"type":"reasoning","summary":[{"type":"summary_text","text":"hidden reasoning"}]}]}),
            json!({"contents":[{"role":"model","parts":[{"text":"assistant text"}]}]}),
            json!({"contents":[{"role":"user","parts":[{"functionResponse":{"name":"f","response":{"secret":"tool output"}}}]}]}),
            json!({"contents":[{"role":"user","parts":[{"inlineData":{"mimeType":"image/png","data":"aGk="}}]}]}),
            // A nested object is a structure, not a sentence.
            json!({"messages":[{"role":"user","content":{"text":"nested"}}]}),
            json!({"instructions":"system text"}),
            json!({"model":"public","prompt":"a completion prompt is not a stored user turn"}),
        ];
        for body in excluded {
            assert_eq!(preview(body.clone()), None, "{body} must not be previewed");
        }
    }

    /// A body with nothing to read reads as nothing — never as a placeholder.
    #[test]
    fn a_trace_preview_has_nothing_to_say_about_a_body_it_cannot_read() {
        assert_eq!(first_user_query(None), None);
        assert_eq!(first_user_query(Some("{ truncated")), None);
        assert_eq!(first_user_query(Some("not json at all")), None);
        assert_eq!(first_user_query(Some("null")), None);
        assert_eq!(first_user_query(Some("[{\"role\":\"user\"}]")), None);
        assert_eq!(preview(json!({"messages":[]})), None);
        assert_eq!(
            preview(json!({"messages":[{"role":"user","content":""}]})),
            None
        );
        assert_eq!(
            preview(json!({"messages":[{"role":"user","content":"   \n\t "}]})),
            None
        );
        assert_eq!(
            preview(json!({"messages":[{"role":"user","content":[]}]})),
            None
        );
        // A redacted body is what the policy stored, so that is what it says.
        assert_eq!(
            preview(json!({"messages":[{"role":"user","content":"[REDACTED]"}]})),
            Some("[REDACTED]".into())
        );
    }

    /// The preview is one line of prose, and it cannot be longer than the bound.
    #[test]
    fn a_trace_preview_is_whitespace_normalized_and_bounded() {
        assert_eq!(
            preview(
                json!({"messages":[{"role":"user","content":"  two\n\nlines\tand   spaces  "}]})
            ),
            Some("two lines and spaces".into())
        );
        let ascii = "a".repeat(1000);
        assert_eq!(preview(json!({"input": ascii})).unwrap().len(), 240);
        // Three bytes per character, so the byte bound cuts at 80 of them — and it
        // cuts between characters, never inside one.
        let cjk = "字".repeat(1000);
        let cut = preview(json!({"input": cjk})).unwrap();
        assert_eq!(cut.chars().count(), 80);
        assert_eq!(cut.len(), 240);
        assert_eq!(
            cut.chars().count(),
            cut.chars().filter(|c| *c == '字').count()
        );
        // A character that would straddle the bound is dropped rather than split.
        let mixed = format!("{}{}", "a".repeat(239), "字");
        let cut = preview(json!({"input": mixed})).unwrap();
        assert_eq!(cut.len(), 239);
        assert!(cut.is_char_boundary(cut.len()));
    }
}
