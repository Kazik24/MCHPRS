use super::super::{PLOT_SECTIONS, PLOT_WIDTH};
use super::repository::button;
use super::snapshot::{state, Snapshot};
use crate::player::PlayerPos;
use anyhow::{ensure, Result};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use serde_json::{json, Value};
use std::collections::BinaryHeap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Marker {
    pub pos: BlockPos,
    pub kind: u8,
}

pub(super) struct Diff {
    pub from_id: String,
    pub to_id: String,
    pub from_label: String,
    pub to_label: String,
    from: Snapshot,
    to: Snapshot,
    sections: Vec<usize>,
    pub counts: [u64; 4],
    runtime_changed: bool,
    pub _reservation: super::Reservation,
}

fn air(id: u32) -> bool {
    matches!(
        Block::from_id(id).get_name(),
        "air" | "void_air" | "cave_air"
    )
}

impl Diff {
    pub fn new(
        from_id: String,
        to_id: String,
        from: Snapshot,
        to: Snapshot,
        reservation: super::Reservation,
    ) -> Result<Self> {
        ensure!(from.plot == to.plot, "Cannot compare different plots");
        let a = from.fingerprints()?;
        let b = to.fingerprints()?;
        let sections = a
            .sections
            .iter()
            .zip(&b.sections)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some(i))
            .collect();
        let mut diff = Self {
            from_id,
            to_id,
            from_label: "From".into(),
            to_label: "To".into(),
            from,
            to,
            sections,
            counts: [0; 4],
            runtime_changed: a.execution != b.execution,
            _reservation: reservation,
        };
        for &section in &diff.sections {
            let chunk = section / PLOT_SECTIONS;
            let sy = section % PLOT_SECTIONS;
            for index in 0..4096 {
                let pos = diff.position(chunk, sy, index);
                if let Some(kind) = diff.kind(pos)? {
                    diff.counts[kind as usize] += 1;
                }
            }
        }
        Ok(diff)
    }

    fn position(&self, chunk: usize, sy: usize, index: usize) -> BlockPos {
        BlockPos::new(
            self.from.plot.0 * PLOT_WIDTH * 16
                + (chunk as i32 / PLOT_WIDTH) * 16
                + (index & 15) as i32,
            sy as i32 * 16 + (index / 256) as i32,
            self.from.plot.1 * PLOT_WIDTH * 16
                + (chunk as i32 % PLOT_WIDTH) * 16
                + ((index / 16) & 15) as i32,
        )
    }

    fn kind(&self, pos: BlockPos) -> Result<Option<u8>> {
        let a = self.from.block(pos);
        let b = self.to.block(pos);
        Ok(if air(a) && air(b) {
            None
        } else if air(a) {
            Some(0)
        } else if air(b) {
            Some(1)
        } else if a != b {
            Some(2)
        } else if self.from.entity(pos)? != self.to.entity(pos)? {
            Some(3)
        } else {
            None
        })
    }

    pub fn summary(&self) -> Value {
        json!({"text":"","extra":[
            button(&format!("{} [{}]",self.from_label,&self.from_id[..8]),&format!("/git show {}",self.from_id)),
            json!({"text":" -> "}),
            button(&format!("{} [{}]",self.to_label,&self.to_id[..8]),&format!("/git show {}",self.to_id)),
            json!({"text":format!("\n{} added, {} removed, {} changed{}\n",self.counts[0],self.counts[1],self.counts[2]+self.counts[3],if self.runtime_changed {"; execution changed"} else {""})}),
            button("Show glow","/git diff show"), button("Hide glow","/git diff hide")
        ]})
    }

    pub fn near(&self, center: PlayerPos, radius: f64, limit: usize) -> Result<Vec<Marker>> {
        let mut sections: Vec<_> = self
            .sections
            .iter()
            .map(|&section| {
                let start = self.position(section / PLOT_SECTIONS, section % PLOT_SECTIONS, 0);
                let distance = [
                    (center.x, start.x),
                    (center.y, start.y),
                    (center.z, start.z),
                ]
                .iter()
                .map(|&(p, s)| (p - p.clamp(s as f64, s as f64 + 16.0)).powi(2))
                .sum::<f64>();
                (distance, section)
            })
            .collect();
        sections.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut heap = BinaryHeap::new();
        if limit == 0 {
            return Ok(Vec::new());
        }
        for (distance, section) in sections {
            if distance > radius * radius {
                break;
            }
            if heap.len() == limit
                && (distance * 1024.0) as i64
                    > heap
                        .peek()
                        .map(|v: &(i64, i32, i32, i32, u8)| v.0)
                        .unwrap_or(i64::MAX)
            {
                break;
            }
            let chunk = section / PLOT_SECTIONS;
            let sy = section % PLOT_SECTIONS;
            for index in 0..4096 {
                let a = state(
                    self.from.data.chunk_data[chunk].sections[sy].as_ref(),
                    index,
                );
                let b = state(self.to.data.chunk_data[chunk].sections[sy].as_ref(), index);
                if air(a) && air(b) {
                    continue;
                }
                let pos = self.position(chunk, sy, index);
                let distance = (center.x - pos.x as f64 - 0.5).powi(2)
                    + (center.y - pos.y as f64 - 0.5).powi(2)
                    + (center.z - pos.z as f64 - 0.5).powi(2);
                if distance > radius * radius {
                    continue;
                }
                let Some(kind) = self.kind(pos)? else {
                    continue;
                };
                let candidate = ((distance * 1024.0) as i64, pos.x, pos.y, pos.z, kind);
                if heap.len() < limit {
                    heap.push(candidate);
                } else if heap.peek().is_some_and(|last| candidate < *last) {
                    heap.pop();
                    heap.push(candidate);
                }
            }
        }
        Ok(heap
            .into_sorted_vec()
            .into_iter()
            .map(|(_, x, y, z, kind)| Marker {
                pos: BlockPos::new(x, y, z),
                kind,
            })
            .collect())
    }

    pub fn inspect(&self, pos: BlockPos, side: Option<&str>) -> Result<Value> {
        ensure!(
            self.kind(pos)?.is_some(),
            "That position has no change in this comparison"
        );
        let a = Block::from_id(self.from.block(pos));
        let b = Block::from_id(self.to.block(pos));
        let description = |block: Block| {
            let mut props: Vec<_> = block
                .properties()
                .into_iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            props.sort();
            format!(
                "{}{}",
                block.get_name(),
                if props.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", props.join(", "))
                }
            )
        };
        let mut text = format!(
            "({}, {}, {})\nFrom: {}\nTo: {}",
            pos.x,
            pos.y,
            pos.z,
            description(a),
            description(b)
        );
        let ae = self.from.entity(pos)?;
        let be = self.to.entity(pos)?;
        if ae != be {
            text.push_str("\nBlock data changed.");
        }
        if let Some(side) = side {
            ensure!(
                matches!(side, "from" | "to"),
                "Details side must be from or to"
            );
            let data = if side == "from" { ae } else { be };
            let serialized = serde_json::to_string_pretty(&data)?;
            let truncated = serialized.chars().count() > 3000;
            text.push_str(&format!(
                "\n{side} data:\n{}{}",
                serialized.chars().take(3000).collect::<String>(),
                if truncated { "\n[Data truncated]" } else { "" }
            ));
        }
        Ok(
            json!({"text":text,"extra":[button("From details",&format!("/git diff inspect {} {} {} from",pos.x,pos.y,pos.z)),button("To details",&format!("/git diff inspect {} {} {} to",pos.x,pos.y,pos.z))]}),
        )
    }
}

pub(super) fn aimed(
    eye: PlayerPos,
    yaw: f32,
    pitch: f32,
    markers: impl Iterator<Item = Marker>,
    radius: f64,
) -> Option<BlockPos> {
    if !eye.is_valid() || !yaw.is_finite() || !pitch.is_finite() {
        return None;
    }
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    let direction = [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    let origin = [eye.x, eye.y, eye.z];
    let mut best = None;
    let mut closest = radius;
    for marker in markers {
        let start = [marker.pos.x, marker.pos.y, marker.pos.z];
        let mut near: f64 = 0.0;
        let mut far = radius;
        for axis in 0..3 {
            let min = start[axis] as f64;
            let max = min + 1.0;
            if direction[axis].abs() < 1e-12 {
                if origin[axis] < min || origin[axis] > max {
                    far = -1.0;
                    break;
                }
            } else {
                let a = (min - origin[axis]) / direction[axis];
                let b = (max - origin[axis]) / direction[axis];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
        }
        if near <= far && near < closest {
            closest = near;
            best = Some(marker.pos);
        }
    }
    best
}
