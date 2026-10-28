use super::*;
use crate::redpiler::program_graph::{BoundaryDirection, BoundaryOwner, BoundaryPort, BoundaryPortKind, RouteId, RouteOccurrence};

#[test]
fn classification_preserves_direction_and_original_occurrence() {
    let occurrence = RouteOccurrence {
        route: RouteId::new(4),
        ordinal: 2,
    };
    let row = |direction, kind| BoundaryPort {
        owner: BoundaryOwner::Cell(0),
        direction,
        kind,
    };
    assert_eq!(
        port_role(row(BoundaryDirection::Input, BoundaryPortKind::Power { link: 7 })),
        PortRole::Read
    );
    assert_eq!(
        port_role(row(BoundaryDirection::Input, BoundaryPortKind::Callback { occurrence })),
        PortRole::Event
    );
    assert_eq!(
        port_role(row(BoundaryDirection::Output, BoundaryPortKind::Callback { occurrence })),
        PortRole::Effect
    );
    assert_eq!(
        port_role(row(BoundaryDirection::Output, BoundaryPortKind::Power { link: 7 })),
        PortRole::Effect
    );
}
