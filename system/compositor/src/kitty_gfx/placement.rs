//! TTY image placements — cursor-position-based kitty graphics within TTY entities.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::store::{DeletePlacementRequest, KittyGfxImageStore};
use crate::id_map::IdMap;
use crate::props::tty::TtyProps;

/// A single image placement entry within a TTY.
struct PlacementEntry {
    /// The spawned entity for this placement (ImageNode + Node).
    entity: Entity,
    /// Column where the image is placed.
    col: usize,
    /// Row where the image is placed.
    row: usize,
    /// X pixel offset within the cell.
    x_offset: u32,
    /// Y pixel offset within the cell.
    y_offset: u32,
    /// Z-index for stacking.
    z_index: i32,
    /// Display columns (if cell-relative sizing was used).
    columns: Option<u32>,
    /// Display rows (if cell-relative sizing was used).
    rows: Option<u32>,
}

/// Component tracking image placements within a TTY entity.
///
/// Keyed by `(image_id, placement_id)` where placement_id defaults to 0.
#[derive(Component, Default)]
pub struct TtyPlacements {
    entries: HashMap<(u32, u32), PlacementEntry>,
    /// Last computed cell width (for resize repositioning).
    last_cell_w: f32,
    /// Last computed cell height (for resize repositioning).
    last_cell_h: f32,
}

impl TtyPlacements {
    /// Remove all entries, returning entities to despawn.
    fn clear(&mut self) -> Vec<Entity> {
        self.entries
            .drain()
            .map(|(_, entry)| entry.entity)
            .collect()
    }

    /// Remove entries matching an image ID (and optionally placement ID).
    fn remove_by_image(&mut self, image_id: u32, placement_id: Option<u32>) -> Vec<Entity> {
        if let Some(pid) = placement_id {
            // Remove specific placement
            self.entries
                .remove(&(image_id, pid))
                .map(|e| vec![e.entity])
                .unwrap_or_default()
        } else {
            // Remove all placements for this image
            let keys: Vec<_> = self
                .entries
                .keys()
                .filter(|(iid, _)| *iid == image_id)
                .copied()
                .collect();
            keys.into_iter()
                .filter_map(|k| self.entries.remove(&k).map(|e| e.entity))
                .collect()
        }
    }

    /// Remove entries matching a z-index.
    fn remove_by_z_index(&mut self, z: i32) -> Vec<Entity> {
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.z_index == z)
            .map(|(k, _)| *k)
            .collect();
        keys.into_iter()
            .filter_map(|k| self.entries.remove(&k).map(|e| e.entity))
            .collect()
    }
}

/// PostUpdate system: processes placement and deletion requests from the kitty
/// graphics store and reconciles them as child entities of TTY nodes.
#[allow(clippy::too_many_arguments)]
pub fn reconcile_tty_placements(
    mut commands: Commands,
    id_map: Res<IdMap>,
    mut store: ResMut<KittyGfxImageStore>,
    mut placements_query: Query<(&TtyProps, &mut TtyPlacements)>,
    mut nodes: Query<&mut Node>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let delete_requests: Vec<_> = store.delete_requests.drain(..).collect();
    let placement_requests: Vec<_> = store.placement_requests.drain(..).collect();

    if delete_requests.is_empty() && placement_requests.is_empty() {
        return;
    }

    // Process deletions
    for req in delete_requests {
        let tty_id = match &req {
            DeletePlacementRequest::All { tty_id } => tty_id,
            DeletePlacementRequest::ById { tty_id, .. } => tty_id,
            DeletePlacementRequest::ByZIndex { tty_id, .. } => tty_id,
        };

        let Some(tty_entity) = id_map.get_entity(tty_id) else {
            continue;
        };
        let Ok((_, mut placements)) = placements_query.get_mut(tty_entity) else {
            continue;
        };

        let to_despawn = match req {
            DeletePlacementRequest::All { .. } => placements.clear(),
            DeletePlacementRequest::ById {
                image_id,
                placement_id,
                ..
            } => placements.remove_by_image(image_id, placement_id),
            DeletePlacementRequest::ByZIndex { z, .. } => placements.remove_by_z_index(z),
        };

        for entity in to_despawn {
            commands.entity(entity).despawn();
        }
    }

    // Process placements
    for req in placement_requests {
        let Some(tty_entity) = id_map.get_entity(&req.tty_id) else {
            warn!("TTY {:?} not found in id_map", req.tty_id);
            continue;
        };
        let Ok((tty_props, mut placements)) = placements_query.get_mut(tty_entity) else {
            warn!("entity {:?} missing TtyPlacements", tty_entity);
            continue;
        };

        let Some(image) = store.get(req.image_id) else {
            warn!("image id={} not found in store", req.image_id);
            continue;
        };

        // Compute cell metrics from tty props
        let resolved = crate::style::resolve_tty_props(tty_props);
        let cell_w = resolved.font_size * 0.5;
        let cell_h = resolved.font_size * 1.2;

        // Compute position
        let left = req.cursor_col as f32 * cell_w + req.x_offset as f32;
        let top = req.cursor_row as f32 * cell_h + req.y_offset as f32;

        // Compute size: use columns/rows if specified, otherwise image dimensions
        let width = req
            .columns
            .map(|c| c as f32 * cell_w)
            .unwrap_or(image.width as f32);
        let height = req
            .rows
            .map(|r| r as f32 * cell_h)
            .unwrap_or(image.height as f32);

        let placement_id = req.placement_id.unwrap_or(0);
        let key = (req.image_id, placement_id);

        // If replacing an existing placement with the same key, despawn old entity
        if let Some(old) = placements.entries.remove(&key) {
            commands.entity(old.entity).despawn();
        }

        // Spawn the placement entity
        let entity = commands
            .spawn((
                ImageNode::new(image.handle.clone()),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left),
                    top: Val::Px(top),
                    width: Val::Px(width),
                    height: Val::Px(height),
                    ..default()
                },
                ZIndex(req.z_index),
                ChildOf(tty_entity),
            ))
            .id();

        placements.entries.insert(
            key,
            PlacementEntry {
                entity,
                col: req.cursor_col,
                row: req.cursor_row,
                x_offset: req.x_offset,
                y_offset: req.y_offset,
                z_index: req.z_index,
                columns: req.columns,
                rows: req.rows,
            },
        );

        placements.last_cell_w = cell_w;
        placements.last_cell_h = cell_h;
    }

    // Resize repositioning: check if cell metrics changed for any TTY with placements
    for (tty_props, mut placements) in &mut placements_query {
        if placements.entries.is_empty() {
            continue;
        }

        let resolved = crate::style::resolve_tty_props(tty_props);
        let cell_w = resolved.font_size * 0.5;
        let cell_h = resolved.font_size * 1.2;

        if (cell_w - placements.last_cell_w).abs() < 0.01
            && (cell_h - placements.last_cell_h).abs() < 0.01
        {
            continue;
        }

        // Cell metrics changed — reposition all placements
        placements.last_cell_w = cell_w;
        placements.last_cell_h = cell_h;

        for entry in placements.entries.values() {
            if let Ok(mut node) = nodes.get_mut(entry.entity) {
                node.left = Val::Px(entry.col as f32 * cell_w + entry.x_offset as f32);
                node.top = Val::Px(entry.row as f32 * cell_h + entry.y_offset as f32);
                if let Some(c) = entry.columns {
                    node.width = Val::Px(c as f32 * cell_w);
                }
                if let Some(r) = entry.rows {
                    node.height = Val::Px(r as f32 * cell_h);
                }
            }
        }
    }

    redraw.write(RequestRedraw);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(entity: Entity, col: usize, row: usize, z: i32) -> PlacementEntry {
        PlacementEntry {
            entity,
            col,
            row,
            x_offset: 0,
            y_offset: 0,
            z_index: z,
            columns: None,
            rows: None,
        }
    }

    #[test]
    fn clear_returns_all_entities() {
        let mut p = TtyPlacements::default();
        let e1 = Entity::from_bits(1);
        let e2 = Entity::from_bits(2);
        p.entries.insert((1, 0), make_entry(e1, 0, 0, 0));
        p.entries.insert((2, 0), make_entry(e2, 5, 5, 0));

        let mut despawned = p.clear();
        despawned.sort();
        assert_eq!(despawned, vec![e1, e2]);
        assert!(p.entries.is_empty());
    }

    #[test]
    fn remove_by_image_specific_placement() {
        let mut p = TtyPlacements::default();
        let e1 = Entity::from_bits(1);
        let e2 = Entity::from_bits(2);
        p.entries.insert((1, 0), make_entry(e1, 0, 0, 0));
        p.entries.insert((1, 1), make_entry(e2, 0, 0, 0));

        let removed = p.remove_by_image(1, Some(0));
        assert_eq!(removed, vec![e1]);
        assert_eq!(p.entries.len(), 1);
        assert!(p.entries.contains_key(&(1, 1)));
    }

    #[test]
    fn remove_by_image_all_placements() {
        let mut p = TtyPlacements::default();
        let e1 = Entity::from_bits(1);
        let e2 = Entity::from_bits(2);
        let e3 = Entity::from_bits(3);
        p.entries.insert((1, 0), make_entry(e1, 0, 0, 0));
        p.entries.insert((1, 1), make_entry(e2, 0, 0, 0));
        p.entries.insert((2, 0), make_entry(e3, 0, 0, 0));

        let mut removed = p.remove_by_image(1, None);
        removed.sort();
        assert_eq!(removed, vec![e1, e2]);
        assert_eq!(p.entries.len(), 1);
        assert!(p.entries.contains_key(&(2, 0)));
    }

    #[test]
    fn remove_by_image_nonexistent() {
        let mut p = TtyPlacements::default();
        let removed = p.remove_by_image(99, Some(0));
        assert!(removed.is_empty());
    }

    #[test]
    fn remove_by_z_index() {
        let mut p = TtyPlacements::default();
        let e1 = Entity::from_bits(1);
        let e2 = Entity::from_bits(2);
        let e3 = Entity::from_bits(3);
        p.entries.insert((1, 0), make_entry(e1, 0, 0, 5));
        p.entries.insert((2, 0), make_entry(e2, 0, 0, 5));
        p.entries.insert((3, 0), make_entry(e3, 0, 0, 10));

        let mut removed = p.remove_by_z_index(5);
        removed.sort();
        assert_eq!(removed, vec![e1, e2]);
        assert_eq!(p.entries.len(), 1);
    }

    #[test]
    fn remove_by_z_index_none_matching() {
        let mut p = TtyPlacements::default();
        let e1 = Entity::from_bits(1);
        p.entries.insert((1, 0), make_entry(e1, 0, 0, 0));

        let removed = p.remove_by_z_index(99);
        assert!(removed.is_empty());
        assert_eq!(p.entries.len(), 1);
    }
}
