use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

static COORDINATES: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}|\((-?\d+),\s*(-?\d+),\s*(-?\d+)\)",
    )
    .unwrap()
});

/// Keep diagnostics readable while letting the normal teleport command enforce permissions.
pub(super) fn component(message: &str) -> Value {
    let mut extra = Vec::new();
    let mut end = 0;
    for captures in COORDINATES.captures_iter(message) {
        let span = captures.get(0).unwrap();
        let first = if captures.get(1).is_some() { 1 } else { 4 };
        let coordinates: Option<Vec<i32>> = (first..first + 3)
            .map(|i| captures[i].parse().ok())
            .collect();
        let Some(coordinates) = coordinates else {
            continue;
        };
        extra.push(json!({"text": &message[end..span.start()]}));
        let [x, y, z] = coordinates.as_slice() else {
            unreachable!()
        };
        extra.push(json!({
            "text": span.as_str(), "color": "aqua", "underlined": true,
            "click_event": {"action": "run_command", "command": format!("/tp {} {} {}", f64::from(*x) + 0.5, i64::from(*y) + 1, f64::from(*z) + 0.5)},
            "hover_event": {"action": "show_text", "value": {"text": "Teleport above this block"}}
        }));
        end = span.end();
    }
    if end == 0 {
        return json!({"text": message, "color": "red"});
    }
    extra.push(json!({"text": &message[end..]}));
    json!({"text": "", "color": "red", "extra": extra})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_coordinate_links_to_its_own_position_without_changing_the_error() {
        let message = "Source BlockPos { x: -20, y: 30, z: 40 } conflicts with (5, 6, -7).";
        let result = component(message);
        let parts = result["extra"].as_array().unwrap();
        assert_eq!(
            parts
                .iter()
                .map(|p| p["text"].as_str().unwrap())
                .collect::<String>(),
            message
        );
        assert_eq!(parts[1]["click_event"]["command"], "/tp -19.5 31 40.5");
        assert_eq!(parts[3]["click_event"]["command"], "/tp 5.5 7 -6.5");
        assert_eq!(
            component("No coordinates"),
            json!({"text": "No coordinates", "color": "red"})
        );
        assert!(component("(999999999999, 1, 2)").get("extra").is_none());
    }
}
