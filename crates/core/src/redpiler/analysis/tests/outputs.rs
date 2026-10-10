use super::*;

#[test]
fn fixed_payloads_retain_dynamic_and_unknown_state_guards() {
    for name in [
        "glass",
        "redstone_lamp",
        "note_block",
        "composter",
        "furnace",
        "barrel",
        "observer",
        "sticky_piston",
        "command_block",
        "end_portal_frame",
    ] {
        assert!(
            !crate::redpiler::analysis::supported_payload(Block::from_name(name).unwrap()),
            "{name}"
        );
    }
    assert!(!crate::redpiler::analysis::supported_payload(
        Block::Unknown {
            id: Block::GoldBlock {}.get_id(),
        }
    ));
    let powered_target = Block::from_id(Block::Target.get_id() + 1);
    assert_eq!(powered_target.property("power"), Some("1"));
    assert!(!crate::redpiler::analysis::supported_payload(
        powered_target
    ));
}
