//! Text components share the target NBT representation on the wire and in block data.

/// NBT storage remains homogeneous in 1.21.5. Mixed lists use compound entries,
/// with non-compounds stored under the reserved empty key (Mojang's list format).
pub fn list(values: Vec<nbt::Value>) -> nbt::Value {
    let mixed = values.first().is_some_and(|first| {
        values
            .iter()
            .any(|value| std::mem::discriminant(value) != std::mem::discriminant(first))
    });
    if !mixed {
        return nbt::Value::List(values);
    }

    let entries = values
        .into_iter()
        .map(|value| match value {
            nbt::Value::Compound(_) => value,
            value => {
                let wrapper = std::collections::HashMap::from([(String::new(), value)]);
                nbt::Value::Compound(wrapper)
            }
        })
        .collect();

    nbt::Value::List(entries)
}

pub fn from_json(text: &str) -> nbt::Value {
    let mut json = serde_json::from_str::<serde_json::Value>(text)
        .unwrap_or_else(|_| serde_json::json!({"text":text}));
    if json.is_array() {
        json = serde_json::json!({"text":"","extra":json});
    }
    json_to_nbt(&json)
}

pub fn to_json(value: &nbt::Value) -> String {
    // Modern NBT strings are literal text, even when they resemble JSON.
    // The schematic importer converts legacy JSON fields using DataVersion.
    nbt_to_json(value, "").to_string()
}

fn json_to_nbt(value: &serde_json::Value) -> nbt::Value {
    match value {
        serde_json::Value::String(text) => nbt::Value::String(text.clone()),
        serde_json::Value::Bool(value) => nbt::Value::Byte(*value as i8),
        serde_json::Value::Number(number) => {
            let value = number.as_i64().unwrap_or(0) as i32;
            nbt::Value::Int(value)
        }
        serde_json::Value::Array(values) => {
            let entries = values.iter().map(json_to_nbt).collect();
            list(entries)
        }
        serde_json::Value::Object(object) => {
            let entries = object
                .iter()
                .map(|(key, value)| (key.clone(), json_to_nbt(value)))
                .collect();
            nbt::Value::Compound(entries)
        }
        _ => nbt::Value::String(String::new()),
    }
}

fn nbt_to_json(value: &nbt::Value, key: &str) -> serde_json::Value {
    match value {
        nbt::Value::String(text) => text.clone().into(),
        nbt::Value::Byte(value)
            if matches!(
                key,
                "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated"
            ) =>
        {
            (*value != 0).into()
        }
        nbt::Value::Byte(value) => (*value).into(),
        nbt::Value::Int(value) => (*value).into(),
        nbt::Value::Compound(object) if object.len() == 1 && object.contains_key("") => {
            nbt_to_json(&object[""], key)
        }
        nbt::Value::Compound(object) => {
            let entries = object
                .iter()
                .map(|(key, value)| (key.clone(), nbt_to_json(value, key)))
                .collect();
            serde_json::Value::Object(entries)
        }
        nbt::Value::List(values) => {
            let entries = values.iter().map(|value| nbt_to_json(value, "")).collect();
            serde_json::Value::Array(entries)
        }
        _ => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::PacketEncoderExt;

    #[test]
    fn numeric_and_boolean_text_stays_text_on_roundtrip() {
        for text in ["32", "true", "false", "null", "-1"] {
            let original = nbt::Value::String(text.into());
            assert_eq!(from_json(&to_json(&original)), original);
        }
        for text in ["\"quoted text\"", "{\"text\":\"literal\"}"] {
            let literal = nbt::Value::String(text.into());
            assert_eq!(from_json(&to_json(&literal)), literal);
        }
    }

    #[test]
    fn nested_mixed_text_lists_encode_and_keep_style_and_translation_arguments() {
        let json = serde_json::json!({
            "text": "heading", "bold": true,
            "extra": ["plain", {"text": "styled", "color": "red", "extra": ["", {"text": "nested"}]}],
            "with": ["argument", {"text": "second", "italic": false}]
        });
        let value = from_json(&json.to_string());
        let nbt::Value::Compound(c) = &value else {
            panic!("not a compound")
        };
        let nbt::Value::List(extra) = &c["extra"] else {
            panic!("not a list")
        };
        assert!(extra.iter().all(|v| matches!(v, nbt::Value::Compound(_))));
        let nbt::Value::Compound(wrapper) = &extra[0] else {
            unreachable!()
        };
        assert_eq!(wrapper[""], nbt::Value::String("plain".into()));
        let blob = nbt::Blob::with_content(c.clone());
        let mut named = Vec::new();
        blob.to_writer(&mut named).unwrap();
        let decoded = nbt::Blob::from_reader(&mut std::io::Cursor::new(named)).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&to_json(&nbt::Value::Compound(
                decoded.content
            )))
            .unwrap(),
            json
        );
        let mut wire = Vec::new();
        wire.write_text(&json.to_string());
        assert_eq!(wire[0], 10);
    }

    #[test]
    fn homogeneous_lists_keep_their_original_types() {
        for values in [
            vec![],
            vec![nbt::Value::String("a".into()); 4],
            vec![nbt::Value::Int(3); 2],
        ] {
            assert_eq!(list(values.clone()), nbt::Value::List(values));
        }
        assert_eq!(
            to_json(&nbt::Value::Compound(std::collections::HashMap::from([(
                String::new(),
                nbt::Value::String("empty row".into())
            )]))),
            "\"empty row\""
        );
    }
}
