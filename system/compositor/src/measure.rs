//! Measure request handling — responds to `?measure` with computed dimensions.
//!
//! Requests are buffered in PreUpdate (by `process_commands`) and processed
//! in PostUpdate after layout, so responses contain current-frame dimensions.

use bevy::prelude::*;
use bevy::ui::UiSystems;

use crate::id_map::IdMap;
use crate::io::StdoutEmitter;
use crate::resize::compute_dimensions;

pub struct MeasurePlugin;

impl Plugin for MeasurePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<MeasureRequest>()
            .add_systems(PostUpdate, respond_to_measures.after(UiSystems::Prepare));
    }
}

/// A buffered `?measure` request awaiting layout completion.
#[derive(Message)]
pub struct MeasureRequest {
    pub target: String,
    pub seq: u64,
}

/// PostUpdate system: responds to buffered `?measure` requests with
/// computed dimensions from the current frame's layout pass.
fn respond_to_measures(
    mut requests: MessageReader<MeasureRequest>,
    id_map: Res<IdMap>,
    computed_nodes: Query<&ComputedNode>,
    emitter: Res<StdoutEmitter>,
) {
    for req in requests.read() {
        if let Some(entity) = id_map.get_entity(&req.target)
            && let Ok(computed) = computed_nodes.get(entity)
        {
            let (w, h, cw, ch) = compute_dimensions(computed);
            let seq = req.seq;
            let w = format!("{w:.1}");
            let h = format!("{h:.1}");
            let cw = format!("{cw:.1}");
            let ch = format!("{ch:.1}");
            emitter.frame(|em| {
                byo::byo_write!(em,
                    .measure {seq} width={w} height={h} content-width={cw} content-height={ch}
                )
            });
        }
    }
}
