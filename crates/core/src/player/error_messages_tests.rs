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
