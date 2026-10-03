//! Text components share the target NBT representation on the wire and in block data.
pub fn from_json(text: &str) -> nbt::Value {
    fn convert(v: &serde_json::Value) -> nbt::Value {
        match v {
            serde_json::Value::String(s) => nbt::Value::String(s.clone()),
            serde_json::Value::Bool(b) => nbt::Value::Byte(*b as i8),
            serde_json::Value::Number(n) => nbt::Value::Int(n.as_i64().unwrap_or(0) as i32),
            serde_json::Value::Array(a) => nbt::Value::List(a.iter().map(convert).collect()),
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
            nbt::Value::Compound(c) => serde_json::Value::Object(
                c.iter().map(|(k, v)| (k.clone(), convert(v, k))).collect(),
            ),
            nbt::Value::List(l) => {
                serde_json::Value::Array(l.iter().map(|v| convert(v, "")).collect())
            }
            _ => serde_json::Value::Null,
        }
    }
    // Earlier schematics stored JSON strings; retain those verbatim internally.
    if let nbt::Value::String(s) = value {
        if serde_json::from_str::<serde_json::Value>(s).is_ok() {
            return s.clone();
        }
    }
    convert(value, "").to_string()
}
