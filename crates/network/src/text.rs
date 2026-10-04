//! Text components share the target NBT representation on the wire and in block data.

/// NBT storage remains homogeneous in 1.21.5. Mixed lists use compound entries,
/// with non-compounds stored under the reserved empty key (Mojang's list format).
pub fn list(values: Vec<nbt::Value>) -> nbt::Value {
    let mixed = values.first().is_some_and(|first| {
        values
            .iter()
            .any(|value| std::mem::discriminant(value) != std::mem::discriminant(first))
    });
    nbt::Value::List(if mixed {
        values
            .into_iter()
            .map(|value| match value {
                nbt::Value::Compound(_) => value,
                value => {
                    nbt::Value::Compound(std::collections::HashMap::from([(String::new(), value)]))
                }
            })
            .collect()
    } else {
        values
    })
}
pub fn from_json(text: &str) -> nbt::Value {
    fn convert(v: &serde_json::Value) -> nbt::Value {
        match v {
            serde_json::Value::String(s) => nbt::Value::String(s.clone()),
            serde_json::Value::Bool(b) => nbt::Value::Byte(*b as i8),
            serde_json::Value::Number(n) => nbt::Value::Int(n.as_i64().unwrap_or(0) as i32),
            serde_json::Value::Array(a) => list(a.iter().map(convert).collect()),
            serde_json::Value::Object(o) => {
                nbt::Value::Compound(o.iter().map(|(k, v)| (k.clone(), convert(v))).collect())
            }
            _ => nbt::Value::String(String::new()),
        }
    }
    let mut json = serde_json::from_str::<serde_json::Value>(text)
        .unwrap_or_else(|_| serde_json::json!({"text":text}));
    if json.is_array() {
        json = serde_json::json!({"text":"","extra":json});
    }
    convert(&json)
}
pub fn to_json(value: &nbt::Value) -> String {
    fn convert(v: &nbt::Value, key: &str) -> serde_json::Value {
        match v {
            nbt::Value::String(s) => s.clone().into(),
            nbt::Value::Byte(b)
                if matches!(
                    key,
                    "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated"
                ) =>
            {
                (*b != 0).into()
            }
            nbt::Value::Byte(b) => (*b).into(),
            nbt::Value::Int(i) => (*i).into(),
            nbt::Value::Compound(c) if c.len() == 1 && c.contains_key("") => convert(&c[""], key),
            nbt::Value::Compound(c) => serde_json::Value::Object(
                c.iter().map(|(k, v)| (k.clone(), convert(v, k))).collect(),
            ),
            nbt::Value::List(l) => {
                serde_json::Value::Array(l.iter().map(|v| convert(v, "")).collect())
            }
            _ => serde_json::Value::Null,
        }
    }
    // Modern NBT strings are literal text, even when they resemble JSON.
    // The schematic importer converts legacy JSON fields using DataVersion.
    convert(value, "").to_string()
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
