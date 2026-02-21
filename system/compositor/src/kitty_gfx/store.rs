//! Kitty image store — maps image IDs to Bevy GPU texture handles.

use bevy::prelude::*;
use std::collections::HashMap;

/// A stored kitty graphics image with its Bevy texture handle.
pub struct KittyGfxImage {
    /// Bevy image asset handle (GPU-side texture).
    pub handle: Handle<Image>,
    /// Source pixel width.
    pub width: u32,
    /// Source pixel height.
    pub height: u32,
}

/// In-progress multi-chunk upload.
pub struct PendingUpload {
    /// Image ID being uploaded.
    pub image_id: u32,
    /// Accumulated base64 payload across chunks.
    pub payload: Vec<u8>,
    /// Pixel format for the final image.
    pub format: byo::kitty_gfx::Format,
    /// Source pixel width (from first chunk).
    pub width: Option<u32>,
    /// Source pixel height (from first chunk).
    pub height: Option<u32>,
    /// Compression method.
    pub compression: byo::kitty_gfx::Compression,
    /// Original action (needed to know if TransmitDisplay at finalize time).
    pub action: byo::kitty_gfx::Action,
    /// Placement ID from the first chunk.
    pub placement_id: Option<u32>,
    /// Display columns from the first chunk.
    pub columns: Option<u32>,
    /// Display rows from the first chunk.
    pub rows: Option<u32>,
    /// X pixel offset from the first chunk.
    pub x_offset: Option<u32>,
    /// Y pixel offset from the first chunk.
    pub y_offset: Option<u32>,
    /// Z-index from the first chunk.
    pub z_index: Option<i32>,
    /// Cursor position captured when upload started.
    pub cursor_pos: (usize, usize),
    /// TTY target captured when upload started.
    pub tty_target: String,
}

/// Central store for kitty graphics images.
///
/// Maps kitty image IDs (`i=N`) to Bevy `Handle<Image>` resources.
/// Handles multi-chunk uploads and image number → ID mappings.
#[derive(Resource, Default)]
pub struct KittyGfxImageStore {
    /// Completed images indexed by kitty image ID.
    images: HashMap<u32, KittyGfxImage>,
    /// Optional image number (`I=N`) → image ID mapping.
    number_to_id: HashMap<u32, u32>,
    /// Currently in-progress chunked upload (at most one at a time).
    pub pending: Option<PendingUpload>,
    /// Auto-incrementing ID for images that don't specify one.
    next_auto_id: u32,
    /// Pending placement requests from transmit+display and put commands.
    pub placement_requests: Vec<PlacementRequest>,
    /// Pending deletion requests from delete commands.
    pub delete_requests: Vec<DeletePlacementRequest>,
}

/// Request to place an image at a cursor position within a TTY.
pub struct PlacementRequest {
    /// ID of the TTY entity that owns this placement.
    pub tty_id: String,
    /// Kitty image ID.
    pub image_id: u32,
    /// Optional placement ID (for replacing specific placements).
    pub placement_id: Option<u32>,
    /// Cursor column at the time of placement.
    pub cursor_col: usize,
    /// Cursor row at the time of placement.
    pub cursor_row: usize,
    /// Display columns (`c=`).
    pub columns: Option<u32>,
    /// Display rows (`r=`).
    pub rows: Option<u32>,
    /// X pixel offset within cell (`X=`).
    pub x_offset: u32,
    /// Y pixel offset within cell (`Y=`).
    pub y_offset: u32,
    /// Z-index for stacking order (`z=`).
    pub z_index: i32,
}

/// Request to delete placements from a TTY.
pub enum DeletePlacementRequest {
    /// Delete all placements in a TTY.
    All { tty_id: String },
    /// Delete placements by image ID (and optionally placement ID).
    ById {
        tty_id: String,
        image_id: u32,
        placement_id: Option<u32>,
    },
    /// Delete placements by z-index.
    ByZIndex { tty_id: String, z: i32 },
}

impl KittyGfxImageStore {
    /// Get or assign an image ID. If the command specifies `i=N`, use that.
    /// Otherwise auto-assign a unique ID starting from 1.
    pub fn resolve_id(&mut self, cmd: &byo::kitty_gfx::KittyGfxCommand) -> u32 {
        if let Some(id) = cmd.image_id
            && id > 0
        {
            // Track the max seen for auto-assign
            if id >= self.next_auto_id {
                self.next_auto_id = id + 1;
            }
            return id;
        }
        // Auto-assign
        if self.next_auto_id == 0 {
            self.next_auto_id = 1;
        }
        let id = self.next_auto_id;
        self.next_auto_id += 1;
        id
    }

    /// Store a completed image.
    pub fn insert(&mut self, id: u32, image: KittyGfxImage) {
        self.images.insert(id, image);
    }

    /// Register an image number → ID mapping.
    pub fn set_number(&mut self, number: u32, id: u32) {
        self.number_to_id.insert(number, id);
    }

    /// Look up an image by ID.
    pub fn get(&self, id: u32) -> Option<&KittyGfxImage> {
        self.images.get(&id)
    }

    /// Look up an image by image number, returning both the resolved ID and image.
    pub fn get_by_number(&self, number: u32) -> Option<(u32, &KittyGfxImage)> {
        self.number_to_id
            .get(&number)
            .and_then(|&id| self.images.get(&id).map(|img| (id, img)))
    }

    /// Remove an image by ID. Returns true if it existed.
    pub fn remove(&mut self, id: u32) -> bool {
        self.images.remove(&id).is_some()
    }

    /// Remove all images.
    pub fn clear(&mut self) {
        self.images.clear();
        self.number_to_id.clear();
    }
}
