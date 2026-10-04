use serde_json::{json, Map, Value};

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

// Preserve supported component structure and inheritance; discard unsupported fields.
fn component(value: &Value, depth: usize) -> Result<Value, String> {
    if depth > 64 {
        return Err("Text component nesting is too deep".into());
    }
    Ok(match value {
        Value::String(text) => json!({"text":text}),
        Value::Array(values) => {
            // Minecraft arrays inherit the first component's style for subsequent siblings.
            let mut values = values.iter();
            let mut first = values
                .next()
                .map(|v| component(v, depth + 1))
                .transpose()?
                .unwrap_or_else(|| json!({"text":""}));
            let remaining = values
                .map(|value| component(value, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            if !remaining.is_empty() {
                let extra = first
                    .as_object_mut()
                    .unwrap()
                    .entry("extra")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .unwrap();
                extra.extend(remaining);
            }
            first
        }
        Value::Object(object) => {
            let mut result = Map::new();
            result.insert(
                "text".into(),
                object
                    .get("text")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("fallback").and_then(Value::as_str))
                    .unwrap_or("")
                    .into(),
            );
            for key in [
                "bold",
                "italic",
                "underlined",
                "strikethrough",
                "obfuscated",
            ] {
                if let Some(value) = object.get(key).and_then(Value::as_bool) {
                    result.insert(key.into(), value.into());
                }
            }
            if let Some(color) = object.get("color").and_then(Value::as_str) {
                let valid = matches!(
                    color,
                    "black"
                        | "dark_blue"
                        | "dark_green"
                        | "dark_aqua"
                        | "dark_red"
                        | "dark_purple"
                        | "gold"
                        | "gray"
                        | "dark_gray"
                        | "blue"
                        | "green"
                        | "aqua"
                        | "red"
                        | "light_purple"
                        | "yellow"
                        | "white"
                        | "reset"
                ) || (color.len() == 7
                    && color.starts_with('#')
                    && color[1..].bytes().all(|b| b.is_ascii_hexdigit()));
                if valid {
                    result.insert("color".into(), color.into());
                }
            }
            if let Some(Value::Array(extra)) = object
                .get("extra")
                .filter(|value| value.as_array().is_some_and(|extra| !extra.is_empty()))
            {
                result.insert(
                    "extra".into(),
                    Value::Array(
                        extra
                            .iter()
                            .map(|v| component(v, depth + 1))
                            .collect::<Result<_, _>>()?,
                    ),
                );
            }
            Value::Object(result)
        }
        _ => return Err("Expected a text string, object or array".into()),
    })
}

fn target<'a>(input: &'a str, player: Option<&str>) -> Result<(Recipient, &'a str), String> {
    let end = if input.starts_with('@') && input.as_bytes().get(2) == Some(&b'[') {
        input.find(']').ok_or("Unclosed selector options")? + 1
    } else {
        input.find(char::is_whitespace).unwrap_or(input.len())
    };
    let selector = &input[..end];
    let recipient = match selector.split('[').next().unwrap() {
        "@a" => Recipient::All,
        "@s" => player.map_or(Recipient::Nobody, |name| Recipient::Name(name.into())),
        name if !name.starts_with('@') && !name.is_empty() => Recipient::Name(name.into()),
        _ => return Err("Supported targets: @a, @s or a player name".into()),
    };
    Ok((recipient, input[end..].trim_start()))
}

pub(crate) fn parse(
    command: &str,
    source: &str,
    player: Option<&str>,
) -> Result<ChatCommand, String> {
    let command = command
        .trim_start()
        .strip_prefix('/')
        .unwrap_or(command.trim_start());
    let (name, args) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    let args = args.trim_start();
    match name {
        "say" if !args.is_empty() => Ok(ChatCommand {
            recipient: Recipient::All,
            message: json!({"text":format!("[{source}] {args}")}).to_string(),
        }),
        "tellraw" => {
            let (recipient, text) = target(args, player)?;
            let value = serde_json::from_str(text)
                .map_err(|_| "Usage: /tellraw <target> <JSON text>".to_owned())?;
            Ok(ChatCommand {
                recipient,
                message: component(&value, 0)?.to_string(),
            })
        }
        "say" => Err("Usage: /say <message>".into()),
        _ => Err("Only /tellraw and /say are supported in command blocks".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_and_literal_say_preserve_text() {
        let command = parse(
            "/tellraw @a[distance=..300, name=ignored] \"hello\"",
            "@",
            None,
        )
        .unwrap();
        assert_eq!(command.recipient, Recipient::All);
        assert_eq!(command.message, "{\"text\":\"hello\"}");
        assert_eq!(
            parse("tellraw @s \"hi\"", "Bob", Some("Bob"))
                .unwrap()
                .recipient,
            Recipient::Name("Bob".into())
        );
        assert_eq!(
            parse("tellraw @s \"hi\"", "@", None).unwrap().recipient,
            Recipient::Nobody
        );
        assert!(Recipient::Name("Bob".into()).matches("bOB"));
        assert_eq!(
            parse("say  a  b &c!", "Bob", None).unwrap().message,
            "{\"text\":\"[Bob] a  b &c!\"}"
        );
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
    fn formatting_arrays_and_extra_ignore_unsupported_fields() {
        let command = parse(r##"tellraw @a [{"text":"parent","color":"#FFCB17","bold":true,"extra":[{"text":"child","bold":false}]},{"score":{"name":"x","objective":"y"},"extra":["kept"]},{"text":"end","clickEvent":{"action":"run_command","value":"/stop"},"hoverEvent":{}}]"##, "@", None).unwrap();
        let value: Value = serde_json::from_str(&command.message).unwrap();
        assert_eq!(value["color"], "#FFCB17");
        assert_eq!(value["extra"][0]["bold"], false);
        assert_eq!(value["extra"][1]["extra"][0]["text"], "kept");
        assert!(!command.message.contains("score"));
        assert!(!command.message.contains("Event"));
        for input in [
            r#"tellraw @a []"#,
            r#"tellraw @a ["hello"]"#,
            r#"tellraw @a {"text":"hello","extra":[]}"#,
        ] {
            assert!(!parse(input, "@", None).unwrap().message.contains("extra"));
        }
    }
}
