use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Value, json};

static COORDINATES: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}|\((-?\d+),\s*(-?\d+),\s*(-?\d+)\)").unwrap());

/// Keep diagnostics readable while linking block positions to the highlight command.
pub(super) fn component(message: &str) -> Value {
    coordinate_component(message, "red")
}

pub(crate) fn diagnostic_component(message: &str) -> Value {
    coordinate_component(message, "yellow")
}

fn coordinate_component(message: &str, color: &str) -> Value {
    let mut extra = Vec::new();
    let mut end = 0;
    for captures in COORDINATES.captures_iter(message) {
        let span = captures.get(0).unwrap();
        let first = if captures.get(1).is_some() { 1 } else { 4 };
        let coordinates: Option<Vec<i32>> = (first..first + 3).map(|i| captures[i].parse().ok()).collect();
        let Some(coordinates) = coordinates else {
            continue;
        };
        extra.push(json!({"text": &message[end..span.start()]}));
        let [x, y, z] = coordinates.as_slice() else { unreachable!() };
        let highlight = format!("/hl {x} {y} {z}");
        extra.push(json!({
            "text": span.as_str(), "color": "aqua", "underlined": true,
            "click_event": {"action": "run_command", "command": highlight},
            "insertion": format!("{highlight} --tp"),
            "hover_event": {"action": "show_text", "value": {"text": "Click to highlight. Shift-click, then Enter to teleport."}}
        }));
        end = span.end();
    }
    if end == 0 {
        return json!({"text": message, "color": color});
    }
    extra.push(json!({"text": &message[end..]}));
    json!({"text": "", "color": color, "extra": extra})
}

#[cfg(test)]
#[path = "error_messages_tests.rs"]
mod tests;
