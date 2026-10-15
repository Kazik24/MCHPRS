//! Built-in reset clocks share ordinary pending ticks and have no output routes.
use super::*;

impl GraphBackend {
    pub(super) fn arm_builtin_reset(&mut self, node: NodeId, facing: BlockFace, direction: Option<BlockFace>, captured: Option<&ObservedSite>) {
        if self.symbolic_reset_update(node, facing, direction, captured) {
            return;
        }
        let qualifies = match direction {
            Some(direction) => direction == facing,
            None => captured
                .and_then(|site| match site.properties {
                    ObservedProperties::Electrical { powered, .. } => Some(powered),
                    _ => None,
                })
                .unwrap_or_else(|| self.node_state(node).powered),
        };
        if qualifies {
            // schedule already checks pending membership; no second lookup is needed.
            self.schedule(node, 1, TickPriority::Normal);
        }
    }

    pub(super) fn tick_builtin_reset(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        if let Some(result) = self.symbolic_reset_tick(node, false) {
            return result;
        }
        let powered = self.node_state(node).powered;
        if !powered {
            self.schedule(node, 1, TickPriority::Normal);
        }
        // Reset power changes have no output callbacks to traverse.
        self.set_node(node, !powered, u8::from(!powered) * 15, None, false)
    }
}
