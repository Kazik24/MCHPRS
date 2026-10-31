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

// TODO: Move tests to _tests.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_coordinate_links_preserve_the_message_and_each_position() {
        let message = "Source BlockPos { x: -20, y: 30, z: 40 } conflicts with (5, 6, -7).";
        let result = component(message);
        let parts = result["extra"].as_array().unwrap();
        assert_eq!(parts.iter().map(|p| p["text"].as_str().unwrap()).collect::<String>(), message);
        assert_eq!(parts[1]["click_event"]["command"], "/hl -20 30 40");
        assert_eq!(parts[3]["click_event"]["command"], "/hl 5 6 -7");
        assert_eq!(parts[1]["insertion"], "/hl -20 30 40 --tp");
        assert_eq!(parts[3]["insertion"], "/hl 5 6 -7 --tp");
        assert_eq!(
            parts[1]["hover_event"]["value"]["text"],
            "Click to highlight. Shift-click, then Enter to teleport."
        );
        assert_eq!(component("No coordinates"), json!({"text": "No coordinates", "color": "red"}));
        assert!(component("(999999999999, 1, 2)").get("extra").is_none());

        let diagnostic = diagnostic_component("At (-2, 3, 4)");
        assert_eq!(diagnostic["color"], "yellow");
        assert_eq!(diagnostic["extra"][1]["click_event"]["command"], "/hl -2 3 4");
        assert_eq!(diagnostic["extra"][1]["insertion"], "/hl -2 3 4 --tp");
        assert_eq!(
            diagnostic["extra"][1]["hover_event"]["value"]["text"],
            "Click to highlight. Shift-click, then Enter to teleport."
        );
        assert_eq!(diagnostic_component("Summary"), json!({"text": "Summary", "color": "yellow"}));
        let multiple = diagnostic_component("Source (1, 2, 3), target (-4, 5, -6)");
        assert_eq!(multiple["extra"][1]["click_event"]["command"], "/hl 1 2 3");
        assert_eq!(multiple["extra"][3]["click_event"]["command"], "/hl -4 5 -6");
    }
}
