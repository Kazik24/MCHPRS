use crate::messages;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipient {
    All,
    Name(String),
    Nobody,
}

impl Recipient {
    pub(crate) fn matches(&self, username: &str) -> bool {
        match self {
            Self::All => true,
            Self::Name(name) => name.eq_ignore_ascii_case(username),
            Self::Nobody => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatCommand {
    pub recipient: Recipient,
    pub message: String,
}

pub(crate) const COMMAND_OUTPUT_QUEUE_LIMIT: usize = 4096;
const COMMAND_OUTPUT_DELIVERY_RATE: usize = 1024;

pub(crate) struct CommandBlockDelivery {
    window: std::time::Instant,
    count: usize,
}

impl Default for CommandBlockDelivery {
    fn default() -> Self {
        Self {
            window: std::time::Instant::now(),
            count: 0,
        }
    }
}

impl CommandBlockDelivery {
    pub(crate) fn take(&mut self, now: std::time::Instant, queued: usize) -> usize {
        if now.duration_since(self.window) >= std::time::Duration::from_secs(1) {
            self.window = now;
            self.count = 0;
        }
        let count = queued.min(COMMAND_OUTPUT_DELIVERY_RATE - self.count);
        self.count += count;
        count
    }
}

#[derive(Clone, Debug)]
pub struct CommandBlockOutputState {
    pub(crate) window: std::time::Instant,
    pub(crate) count: usize,
    pub(crate) bytes: usize,
    pub(crate) queued: usize,
    pub(crate) limited: bool,
}

impl CommandBlockOutputState {
    pub(crate) fn execute(&mut self, command: &str, source: &str) -> Result<ChatCommand, String> {
        if self.window.elapsed() >= std::time::Duration::from_secs(1) {
            self.window = std::time::Instant::now();
            self.count = 0;
            self.bytes = 0;
        }
        if (self.limited && self.queued >= COMMAND_OUTPUT_QUEUE_LIMIT) || command.len() > 131_068 || source.len() > 256 {
            return Err(messages::COMMAND_BLOCK_OUTPUT_LIMIT.into());
        }
        let message = parse(command, source, None)?;
        if self.limited && message.message.len() > 65_536 {
            return Err(messages::COMMAND_BLOCK_OUTPUT_BYTE_LIMIT.into());
        }
        self.count += 1;
        self.bytes += message.message.len();
        self.queued += 1;
        Ok(message)
    }
}

#[derive(Debug)]
pub struct CommandBlockEmission {
    pub(crate) pos: mchprs_blocks::BlockPos,
    pub(crate) tick: u64,
    pub(crate) message: ChatCommand,
}

// Preserve supported component structure and inheritance; discard unsupported fields.
fn component(value: &Value, depth: usize) -> Result<Value, String> {
    if depth > 64 {
        return Err(messages::TEXT_COMPONENT_NESTING_TOO_DEEP.into());
    }

    match value {
        Value::String(text) => Ok(json!({"text": text})),
        Value::Array(values) => component_array(values, depth),
        Value::Object(object) => component_object(object, depth),
        _ => Err(messages::EXPECTED_TEXT_STRING_OBJECT_OR_ARRAY.into()),
    }
}

fn component_array(values: &[Value], depth: usize) -> Result<Value, String> {
    // Minecraft arrays inherit the first component's style for subsequent siblings.
    let mut values = values.iter();
    let mut first = match values.next() {
        Some(value) => component(value, depth + 1)?,
        None => json!({"text": ""}),
    };
    let remaining = values.map(|value| component(value, depth + 1)).collect::<Result<Vec<_>, _>>()?;
    if remaining.is_empty() {
        return Ok(first);
    }

    let object = first.as_object_mut().unwrap();
    let extra = object.entry("extra").or_insert_with(|| json!([]));
    extra.as_array_mut().unwrap().extend(remaining);

    Ok(first)
}

fn component_object(object: &Map<String, Value>, depth: usize) -> Result<Value, String> {
    let text = object
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| object.get("fallback").and_then(Value::as_str))
        .unwrap_or("");
    let mut result = Map::new();
    result.insert("text".into(), text.into());

    for key in ["bold", "italic", "underlined", "strikethrough", "obfuscated"] {
        if let Some(value) = object.get(key).and_then(Value::as_bool) {
            result.insert(key.into(), value.into());
        }
    }

    if let Some(color) = object.get("color").and_then(Value::as_str) {
        if crate::chat::valid_color(color) {
            result.insert("color".into(), color.into());
        }
    }

    if let Some(Value::Array(extra)) = object.get("extra") {
        if !extra.is_empty() {
            let children = extra.iter().map(|value| component(value, depth + 1)).collect::<Result<Vec<_>, _>>()?;
            result.insert("extra".into(), Value::Array(children));
        }
    }

    Ok(Value::Object(result))
}

fn target<'a>(input: &'a str, player: Option<&str>) -> Result<(Recipient, &'a str), String> {
    let end = if input.starts_with('@') && input.as_bytes().get(2) == Some(&b'[') {
        input.find(']').ok_or(messages::UNCLOSED_SELECTOR_OPTIONS)? + 1
    } else {
        input.find(char::is_whitespace).unwrap_or(input.len())
    };
    let selector = &input[..end];
    let recipient = match selector.split('[').next().unwrap() {
        "@a" => Recipient::All,
        "@s" => player.map_or(Recipient::Nobody, |name| Recipient::Name(name.into())),
        name if !name.starts_with('@') && !name.is_empty() => Recipient::Name(name.into()),
        _ => return Err(messages::SUPPORTED_TARGETS_S_OR_PLAYER_NAME.into()),
    };
    Ok((recipient, input[end..].trim_start()))
}

pub(crate) fn parse(command: &str, source: &str, player: Option<&str>) -> Result<ChatCommand, String> {
    let command = command.trim_start();
    let command = command.strip_prefix('/').unwrap_or(command);
    let (name, args) = command.split_once(char::is_whitespace).unwrap_or((command, ""));
    let args = args.trim_start();
    match name {
        "say" if !args.is_empty() => Ok(ChatCommand {
            recipient: Recipient::All,
            message: json!({"text":format!("[{source}] {args}")}).to_string(),
        }),
        "tellraw" => {
            let (recipient, text) = target(args, player)?;
            let value = serde_json::from_str(text).map_err(|_| messages::USAGE_TELLRAW_TARGET_JSON_TEXT.to_owned())?;
            Ok(ChatCommand {
                recipient,
                message: component(&value, 0)?.to_string(),
            })
        }
        "say" => Err(messages::USAGE_SAY_MESSAGE.into()),
        _ => Err(messages::ONLY_TELLRAW_SAY_SUPPORTED_COMMAND_BLOCKS.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_and_literal_say_preserve_text() {
        let command = parse("/tellraw @a[distance=..300, name=ignored] \"hello\"", "@", None).unwrap();
        assert_eq!(command.recipient, Recipient::All);
        assert_eq!(command.message, "{\"text\":\"hello\"}");
        assert_eq!(
            parse("tellraw @s \"hi\"", "Bob", Some("Bob")).unwrap().recipient,
            Recipient::Name("Bob".into())
        );
        assert_eq!(parse("tellraw @s \"hi\"", "@", None).unwrap().recipient, Recipient::Nobody);
        assert!(Recipient::Name("Bob".into()).matches("bOB"));
        assert_eq!(parse("say  a  b &c!", "Bob", None).unwrap().message, "{\"text\":\"[Bob] a  b &c!\"}");
        for command in [
            "say",
            "tellraw",
            "tellraw @a {",
            "tellraw @a[] true",
            "tellraw @e \"hi\"",
            "tellraw @a[abc \"hi\"",
            "stop",
        ] {
            assert!(parse(command, "@", None).is_err(), "{command}");
        }
    }

    #[test]
    fn colors_and_nesting_limits_preserve_supported_component_behavior() {
        for (color, accepted) in [
            ("red", true),
            ("reset", true),
            ("#ff00AA", true),
            ("RED", false),
            ("#12345", false),
            ("#1234567", false),
            ("#gg0000", false),
            ("#é0000", false),
        ] {
            let text = json!({"text": "hello", "color": color});
            let input = format!("tellraw @a {text}");
            let command = parse(&input, "@", None).unwrap();
            let output: Value = serde_json::from_str(&command.message).unwrap();

            assert_eq!(output["text"], "hello");
            assert_eq!(output.get("color").is_some(), accepted, "{color}");
        }

        let mut text = json!("nested");
        for _ in 0..64 {
            text = Value::Array(vec![text]);
        }
        let input = format!("tellraw @a {text}");
        assert!(parse(&input, "@", None).is_ok());

        text = Value::Array(vec![text]);
        let input = format!("tellraw @a {text}");
        assert_eq!(parse(&input, "@", None).unwrap_err(), messages::TEXT_COMPONENT_NESTING_TOO_DEEP);
    }

    #[test]
    fn authored_text_matching_notices_is_never_translated() {
        for text in ["Command not found!", messages::COMMAND_NOT_FOUND] {
            let say = parse(&format!("say {text}"), "Bob", Some("Bob")).unwrap();
            let value: Value = serde_json::from_str(&say.message).unwrap();
            assert_eq!(value["text"], format!("[Bob] {text}"));

            let component = json!({"text": text, "bold": false, "extra": [{"text": text}]});
            let tellraw = parse(&format!("tellraw @s {component}"), "Bob", Some("Bob")).unwrap();
            assert_eq!(tellraw.recipient, Recipient::Name("Bob".into()));
            assert_eq!(serde_json::from_str::<Value>(&tellraw.message).unwrap(), component);
        }
    }

    #[test]
    fn formatting_arrays_and_extra_ignore_unsupported_fields() {
        let input = concat!(
            r##"tellraw @a [{"text":"parent","color":"#FFCB17","bold":true,"##,
            r#""extra":[{"text":"child","bold":false}]},"#,
            r#"{"score":{"name":"x","objective":"y"},"extra":["kept"]},"#,
            r#"{"text":"end","clickEvent":{"action":"run_command","value":"/stop"},"#,
            r#""hoverEvent":{}}]"#,
        );
        let command = parse(input, "@", None).unwrap();
        let value: Value = serde_json::from_str(&command.message).unwrap();
        assert_eq!(value["color"], "#FFCB17");
        assert_eq!(value["extra"][0]["bold"], false);
        assert_eq!(value["extra"][1]["extra"][0]["text"], "kept");
        assert!(!command.message.contains("score"));
        assert!(!command.message.contains("Event"));
        for input in [r#"tellraw @a []"#, r#"tellraw @a ["hello"]"#, r#"tellraw @a {"text":"hello","extra":[]}"#] {
            assert!(!parse(input, "@", None).unwrap().message.contains("extra"));
        }
    }
    #[test]
    fn command_bursts_are_buffered_and_delivered_in_order_at_1024_per_second() {
        let now = std::time::Instant::now();
        let mut state = CommandBlockOutputState {
            window: now,
            count: 0,
            bytes: 0,
            queued: 0,
            limited: true,
        };
        let mut queue = Vec::new();
        for index in 0..COMMAND_OUTPUT_QUEUE_LIMIT {
            queue.push(state.execute(&format!("say {index}"), "@").unwrap());
        }
        assert!(state.execute("say overflow", "@").is_err());
        let mut delivery = CommandBlockDelivery { window: now, count: 0 };
        let mut received = Vec::new();
        for second in 0..4 {
            let at = now + std::time::Duration::from_secs(second);
            let count = delivery.take(at, queue.len());
            assert_eq!(count, 1024);
            received.extend(queue.drain(..count).map(|message| message.message));
            assert_eq!(delivery.take(at + std::time::Duration::from_millis(999), queue.len()), 0);
        }
        assert!(queue.is_empty());
        for (index, message) in received.iter().enumerate() {
            assert_eq!(message, &format!(r#"{{"text":"[@] {index}"}}"#));
        }
        state.queued = 0;
        assert!(state.execute("say shut", "@").is_ok());
        assert!(state.execute(&format!("say {}", "x".repeat(65_536)), "@").is_err());
    }
}
