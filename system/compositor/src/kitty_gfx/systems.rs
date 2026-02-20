//! Systems for processing kitty graphics commands and creating Bevy image assets.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use bevy::image::{Image, ImageFormat, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use byo::kitty_gfx::{self, Action, Compression, DeleteTarget, Format, Transmission};

use super::store::{
    DeletePlacementRequest, KittyGfxImage, KittyGfxImageStore, PendingUpload, PlacementRequest,
};
use crate::io::StdoutEmitter;

/// Process a single kitty graphics payload (called per-event from the unified stdin loop).
pub fn process_single(
    payload: &[u8],
    store: &mut KittyGfxImageStore,
    images: &mut Assets<Image>,
    emitter: &StdoutEmitter,
    cursor_pos: (usize, usize),
    tty_target: &str,
) {
    let cmd = match kitty_gfx::parse(payload) {
        Ok(cmd) => cmd,
        Err(e) => {
            warn!("kitty graphics parse error: {e}");
            return;
        }
    };

    match cmd.action {
        Action::TransmitDisplay | Action::Transmit => {
            handle_transmit(&cmd, store, images, emitter, cursor_pos, tty_target);
        }
        Action::Put => {
            handle_put(&cmd, store, emitter, cursor_pos, tty_target);
        }
        Action::Delete => {
            handle_delete(&cmd, store, tty_target);
        }
        Action::Query => {
            handle_query(&cmd, emitter);
        }
        _ => {
            debug!("unhandled kitty action: {:?}", cmd.action);
        }
    }
}

/// Handle transmit/transmit+display: decode image data and store as Bevy texture.
fn handle_transmit(
    cmd: &kitty_gfx::KittyGfxCommand,
    store: &mut KittyGfxImageStore,
    images: &mut Assets<Image>,
    emitter: &StdoutEmitter,
    cursor_pos: (usize, usize),
    tty_target: &str,
) {
    // Multi-chunk: accumulate payload
    if cmd.more {
        if let Some(ref mut pending) = store.pending {
            // Continuation chunk — append to existing pending upload.
            // Don't resolve a new ID; use the pending upload's ID.
            pending.payload.extend_from_slice(&cmd.payload);
            // Suppress OK for intermediate chunks to avoid triggering
            // terminal responses that could interfere with the upload.
        } else {
            // First chunk of a new upload
            let id = store.resolve_id(cmd);
            store.pending = Some(PendingUpload {
                image_id: id,
                payload: cmd.payload.clone(),
                format: cmd.format,
                width: cmd.width,
                height: cmd.height,
                compression: cmd.compression,
                action: cmd.action,
                placement_id: cmd.placement_id,
                columns: cmd.columns,
                rows: cmd.rows,
                x_offset: cmd.x_offset,
                y_offset: cmd.y_offset,
                z_index: cmd.z_index,
                cursor_pos,
                tty_target: tty_target.to_string(),
            });
            if cmd.quiet == 0 {
                send_ok(emitter, id, cmd.placement_id);
            }
        }
        return;
    }

    // Final chunk (or single-chunk upload)
    let (
        id,
        payload,
        format,
        width,
        height,
        compression,
        is_transmit_display,
        placement_id,
        columns,
        rows,
        x_offset,
        y_offset,
        z_index,
        place_cursor,
        place_tty,
    ) = if store.pending.is_some() {
        // There's a pending multi-chunk upload.
        // Only finalize it if this command looks like a genuine final
        // chunk (no explicit image_id, or image_id matches pending).
        // Commands with a mismatched image_id are likely stray responses
        // from a terminal's kitty graphics handler — skip them entirely.
        let pending_id = store.pending.as_ref().unwrap().image_id;
        if cmd.image_id.is_some() && cmd.image_id != Some(pending_id) {
            // Stray command with different image ID — ignore it
            // to protect the in-progress upload.
            debug!(
                "ignoring stray command (id={:?}) during pending upload (id={pending_id})",
                cmd.image_id
            );
            return;
        }
        let pending = store.pending.take().unwrap();
        let mut payload = pending.payload;
        payload.extend_from_slice(&cmd.payload);
        (
            pending.image_id,
            payload,
            pending.format,
            pending.width.or(cmd.width),
            pending.height.or(cmd.height),
            pending.compression,
            pending.action == Action::TransmitDisplay,
            pending.placement_id.or(cmd.placement_id),
            pending.columns.or(cmd.columns),
            pending.rows.or(cmd.rows),
            pending.x_offset.or(cmd.x_offset),
            pending.y_offset.or(cmd.y_offset),
            pending.z_index.or(cmd.z_index),
            pending.cursor_pos,
            pending.tty_target,
        )
    } else {
        let id = store.resolve_id(cmd);
        (
            id,
            cmd.payload.clone(),
            cmd.format,
            cmd.width,
            cmd.height,
            cmd.compression,
            cmd.action == Action::TransmitDisplay,
            cmd.placement_id,
            cmd.columns,
            cmd.rows,
            cmd.x_offset,
            cmd.y_offset,
            cmd.z_index,
            cursor_pos,
            tty_target.to_string(),
        )
    };

    // Only direct transmission for now
    if cmd.transmission != Transmission::Direct {
        warn!("unsupported transmission {:?}, ignoring", cmd.transmission);
        if cmd.quiet == 0 {
            send_error(emitter, id, "ENOTSUP: only direct transmission supported");
        }
        return;
    }

    // Base64 decode
    let raw = match BASE64.decode(&payload) {
        Ok(data) => data,
        Err(e) => {
            warn!("base64 decode error: {e}");
            if cmd.quiet < 2 {
                send_error(emitter, id, &format!("EBADDATA: base64 decode error: {e}"));
            }
            return;
        }
    };

    // Decompress if zlib
    let raw = match compression {
        Compression::Zlib => {
            warn!("zlib not yet supported, ignoring");
            if cmd.quiet < 2 {
                send_error(emitter, id, "ENOTSUP: zlib compression not yet supported");
            }
            return;
        }
        Compression::None => raw,
    };

    // Decode image
    let bevy_image = match format {
        Format::Png => match Image::from_buffer(
            &raw,
            ImageType::Format(ImageFormat::Png),
            default(),
            true,
            default(),
            default(),
        ) {
            Ok(img) => img,
            Err(e) => {
                warn!("PNG decode error: {e}");
                if cmd.quiet < 2 {
                    send_error(emitter, id, &format!("EBADDATA: PNG decode error: {e}"));
                }
                return;
            }
        },
        Format::Rgba => {
            let w = width.unwrap_or(0);
            let h = height.unwrap_or(0);
            if w == 0 || h == 0 {
                warn!("RGBA format requires s= and v= dimensions");
                if cmd.quiet < 2 {
                    send_error(emitter, id, "EBADDATA: missing dimensions for raw RGBA");
                }
                return;
            }
            let expected = (w * h * 4) as usize;
            if raw.len() != expected {
                warn!("RGBA size mismatch: expected {expected}, got {}", raw.len());
                if cmd.quiet < 2 {
                    send_error(emitter, id, "EBADDATA: RGBA data size mismatch");
                }
                return;
            }
            Image::new(
                Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                raw,
                TextureFormat::Rgba8UnormSrgb,
                default(),
            )
        }
        Format::Rgb => {
            let w = width.unwrap_or(0);
            let h = height.unwrap_or(0);
            if w == 0 || h == 0 {
                warn!("RGB format requires s= and v= dimensions");
                if cmd.quiet < 2 {
                    send_error(emitter, id, "EBADDATA: missing dimensions for raw RGB");
                }
                return;
            }
            let expected = (w * h * 3) as usize;
            if raw.len() != expected {
                warn!("RGB size mismatch: expected {expected}, got {}", raw.len());
                if cmd.quiet < 2 {
                    send_error(emitter, id, "EBADDATA: RGB data size mismatch");
                }
                return;
            }
            // Convert RGB → RGBA (Bevy needs 4-channel)
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for chunk in raw.chunks_exact(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255);
            }
            Image::new(
                Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                rgba,
                TextureFormat::Rgba8UnormSrgb,
                default(),
            )
        }
    };

    let img_width = bevy_image.width();
    let img_height = bevy_image.height();
    let handle = images.add(bevy_image);

    info!("stored image id={id} ({img_width}x{img_height})");
    store.insert(
        id,
        KittyGfxImage {
            handle,
            width: img_width,
            height: img_height,
        },
    );

    // Register image number mapping if present
    if let Some(number) = cmd.image_number {
        store.set_number(number, id);
    }

    // Send OK response
    if cmd.quiet == 0 {
        send_ok(emitter, id, placement_id);
    }

    // For transmit+display, also create a placement at the cursor position
    if is_transmit_display {
        store.placement_requests.push(PlacementRequest {
            tty_id: place_tty,
            image_id: id,
            placement_id,
            cursor_col: place_cursor.0,
            cursor_row: place_cursor.1,
            columns,
            rows,
            x_offset: x_offset.unwrap_or(0),
            y_offset: y_offset.unwrap_or(0),
            z_index: z_index.unwrap_or(0),
        });
    }
}

/// Handle put: create a placement for an existing image at the cursor position.
///
/// Resolves the image via `i=` (image ID) first, falling back to `I=` (image number).
fn handle_put(
    cmd: &kitty_gfx::KittyGfxCommand,
    store: &mut KittyGfxImageStore,
    emitter: &StdoutEmitter,
    cursor_pos: (usize, usize),
    tty_target: &str,
) {
    let (id, image) = resolve_image(cmd, store);
    if image.is_none() {
        warn!(
            "put for unknown image (i={:?}, I={:?})",
            cmd.image_id, cmd.image_number
        );
        if cmd.quiet < 2 {
            send_error(emitter, id, "ENOENT: image not found");
        }
        return;
    }

    store.placement_requests.push(PlacementRequest {
        tty_id: tty_target.to_string(),
        image_id: id,
        placement_id: cmd.placement_id,
        cursor_col: cursor_pos.0,
        cursor_row: cursor_pos.1,
        columns: cmd.columns,
        rows: cmd.rows,
        x_offset: cmd.x_offset.unwrap_or(0),
        y_offset: cmd.y_offset.unwrap_or(0),
        z_index: cmd.z_index.unwrap_or(0),
    });

    debug!(
        "put for image id={id} at col={} row={}",
        cursor_pos.0, cursor_pos.1
    );
    if cmd.quiet == 0 {
        send_ok(emitter, id, cmd.placement_id);
    }
}

/// Resolve an image from a command, checking `i=` (image ID) first,
/// then falling back to `I=` (image number → ID mapping).
/// Returns `(resolved_id, Option<&KittyGfxImage>)`.
fn resolve_image<'a>(
    cmd: &kitty_gfx::KittyGfxCommand,
    store: &'a KittyGfxImageStore,
) -> (u32, Option<&'a KittyGfxImage>) {
    // Try image ID first
    if let Some(id) = cmd.image_id {
        if let Some(img) = store.get(id) {
            return (id, Some(img));
        }
    }
    // Fall back to image number
    if let Some(number) = cmd.image_number {
        if let Some((id, img)) = store.get_by_number(number) {
            return (id, Some(img));
        }
    }
    (cmd.image_id.unwrap_or(0), None)
}

/// Handle delete: remove images from the store and push deletion requests.
///
/// Supported targets: `All`, `ById` (with optional placement), `ByZIndex`.
/// Not yet implemented: `AtCursor`, `ByNumber`, `ByPlacement` — these log
/// at debug level and are silently ignored.
fn handle_delete(
    cmd: &kitty_gfx::KittyGfxCommand,
    store: &mut KittyGfxImageStore,
    tty_target: &str,
) {
    match cmd.delete {
        Some(DeleteTarget::All { .. }) => {
            info!("deleting all images");
            store.delete_requests.push(DeletePlacementRequest::All {
                tty_id: tty_target.to_string(),
            });
            store.clear();
        }
        Some(DeleteTarget::ById { id, placement, .. }) => {
            store.delete_requests.push(DeletePlacementRequest::ById {
                tty_id: tty_target.to_string(),
                image_id: id,
                placement_id: placement,
            });
            // Only remove from image store if no specific placement targeted
            if placement.is_none() && store.remove(id) {
                debug!("deleted image id={id}");
            }
        }
        Some(DeleteTarget::ByZIndex { z, .. }) => {
            store
                .delete_requests
                .push(DeletePlacementRequest::ByZIndex {
                    tty_id: tty_target.to_string(),
                    z,
                });
        }
        _ => {
            debug!("unhandled delete target {:?}", cmd.delete);
        }
    }
}

/// Handle query: respond with OK to indicate graphics support.
fn handle_query(cmd: &kitty_gfx::KittyGfxCommand, emitter: &StdoutEmitter) {
    let id = cmd.image_id.unwrap_or(0);
    if cmd.quiet < 2 {
        send_ok(emitter, id, None);
    }
    debug!("query response sent for id={id}");
}

/// Send an OK response for a kitty graphics command.
fn send_ok(emitter: &StdoutEmitter, image_id: u32, placement_id: Option<u32>) {
    let response = match placement_id {
        Some(p) => format!("i={image_id},p={p};OK"),
        None => format!("i={image_id};OK"),
    };
    emitter.kitty_gfx_frame(response.as_bytes());
}

/// Send an error response for a kitty graphics command.
fn send_error(emitter: &StdoutEmitter, image_id: u32, message: &str) {
    let response = format!("i={image_id};{message}");
    emitter.kitty_gfx_frame(response.as_bytes());
}
