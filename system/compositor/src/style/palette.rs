//! Tailwind CSS v3 color palette.
//!
//! Provides the complete set of 22 color families, each with 11 shades
//! (50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950), matching
//! the official Tailwind v3 hex values exactly.

use bevy::prelude::*;

/// Look up a Tailwind v3 color by family name and shade.
///
/// Returns `None` if the name or shade is not recognized.
///
/// # Examples
///
/// ```ignore
/// let color = tailwind_color("blue", 500); // #3b82f6
/// ```
pub fn tailwind_color(name: &str, shade: u16) -> Option<Color> {
    match (name, shade) {
        // ── slate ───────────────────────────────────────────────
        ("slate", 50) => Some(Color::srgba_u8(0xf8, 0xfa, 0xfc, 255)),
        ("slate", 100) => Some(Color::srgba_u8(0xf1, 0xf5, 0xf9, 255)),
        ("slate", 200) => Some(Color::srgba_u8(0xe2, 0xe8, 0xf0, 255)),
        ("slate", 300) => Some(Color::srgba_u8(0xcb, 0xd5, 0xe1, 255)),
        ("slate", 400) => Some(Color::srgba_u8(0x94, 0xa3, 0xb8, 255)),
        ("slate", 500) => Some(Color::srgba_u8(0x64, 0x74, 0x8b, 255)),
        ("slate", 600) => Some(Color::srgba_u8(0x47, 0x55, 0x69, 255)),
        ("slate", 700) => Some(Color::srgba_u8(0x33, 0x41, 0x55, 255)),
        ("slate", 800) => Some(Color::srgba_u8(0x1e, 0x29, 0x3b, 255)),
        ("slate", 900) => Some(Color::srgba_u8(0x0f, 0x17, 0x2a, 255)),
        ("slate", 950) => Some(Color::srgba_u8(0x02, 0x06, 0x17, 255)),

        // ── gray ────────────────────────────────────────────────
        ("gray", 50) => Some(Color::srgba_u8(0xf9, 0xfa, 0xfb, 255)),
        ("gray", 100) => Some(Color::srgba_u8(0xf3, 0xf4, 0xf6, 255)),
        ("gray", 200) => Some(Color::srgba_u8(0xe5, 0xe7, 0xeb, 255)),
        ("gray", 300) => Some(Color::srgba_u8(0xd1, 0xd5, 0xdb, 255)),
        ("gray", 400) => Some(Color::srgba_u8(0x9c, 0xa3, 0xaf, 255)),
        ("gray", 500) => Some(Color::srgba_u8(0x6b, 0x72, 0x80, 255)),
        ("gray", 600) => Some(Color::srgba_u8(0x4b, 0x55, 0x63, 255)),
        ("gray", 700) => Some(Color::srgba_u8(0x37, 0x41, 0x51, 255)),
        ("gray", 800) => Some(Color::srgba_u8(0x1f, 0x29, 0x37, 255)),
        ("gray", 900) => Some(Color::srgba_u8(0x11, 0x18, 0x27, 255)),
        ("gray", 950) => Some(Color::srgba_u8(0x03, 0x07, 0x12, 255)),

        // ── zinc ────────────────────────────────────────────────
        ("zinc", 50) => Some(Color::srgba_u8(0xfa, 0xfa, 0xfa, 255)),
        ("zinc", 100) => Some(Color::srgba_u8(0xf4, 0xf4, 0xf5, 255)),
        ("zinc", 200) => Some(Color::srgba_u8(0xe4, 0xe4, 0xe7, 255)),
        ("zinc", 300) => Some(Color::srgba_u8(0xd4, 0xd4, 0xd8, 255)),
        ("zinc", 400) => Some(Color::srgba_u8(0xa1, 0xa1, 0xaa, 255)),
        ("zinc", 500) => Some(Color::srgba_u8(0x71, 0x71, 0x7a, 255)),
        ("zinc", 600) => Some(Color::srgba_u8(0x52, 0x52, 0x5b, 255)),
        ("zinc", 700) => Some(Color::srgba_u8(0x3f, 0x3f, 0x46, 255)),
        ("zinc", 800) => Some(Color::srgba_u8(0x27, 0x27, 0x2a, 255)),
        ("zinc", 900) => Some(Color::srgba_u8(0x18, 0x18, 0x1b, 255)),
        ("zinc", 950) => Some(Color::srgba_u8(0x09, 0x09, 0x0b, 255)),

        // ── neutral ─────────────────────────────────────────────
        ("neutral", 50) => Some(Color::srgba_u8(0xfa, 0xfa, 0xfa, 255)),
        ("neutral", 100) => Some(Color::srgba_u8(0xf5, 0xf5, 0xf5, 255)),
        ("neutral", 200) => Some(Color::srgba_u8(0xe5, 0xe5, 0xe5, 255)),
        ("neutral", 300) => Some(Color::srgba_u8(0xd4, 0xd4, 0xd4, 255)),
        ("neutral", 400) => Some(Color::srgba_u8(0xa3, 0xa3, 0xa3, 255)),
        ("neutral", 500) => Some(Color::srgba_u8(0x73, 0x73, 0x73, 255)),
        ("neutral", 600) => Some(Color::srgba_u8(0x52, 0x52, 0x52, 255)),
        ("neutral", 700) => Some(Color::srgba_u8(0x40, 0x40, 0x40, 255)),
        ("neutral", 800) => Some(Color::srgba_u8(0x26, 0x26, 0x26, 255)),
        ("neutral", 900) => Some(Color::srgba_u8(0x17, 0x17, 0x17, 255)),
        ("neutral", 950) => Some(Color::srgba_u8(0x0a, 0x0a, 0x0a, 255)),

        // ── stone ───────────────────────────────────────────────
        ("stone", 50) => Some(Color::srgba_u8(0xfa, 0xfa, 0xf9, 255)),
        ("stone", 100) => Some(Color::srgba_u8(0xf5, 0xf5, 0xf4, 255)),
        ("stone", 200) => Some(Color::srgba_u8(0xe7, 0xe5, 0xe4, 255)),
        ("stone", 300) => Some(Color::srgba_u8(0xd6, 0xd3, 0xd1, 255)),
        ("stone", 400) => Some(Color::srgba_u8(0xa8, 0xa2, 0x9e, 255)),
        ("stone", 500) => Some(Color::srgba_u8(0x78, 0x71, 0x6c, 255)),
        ("stone", 600) => Some(Color::srgba_u8(0x57, 0x53, 0x4e, 255)),
        ("stone", 700) => Some(Color::srgba_u8(0x44, 0x40, 0x3c, 255)),
        ("stone", 800) => Some(Color::srgba_u8(0x29, 0x25, 0x24, 255)),
        ("stone", 900) => Some(Color::srgba_u8(0x1c, 0x19, 0x17, 255)),
        ("stone", 950) => Some(Color::srgba_u8(0x0c, 0x0a, 0x09, 255)),

        // ── red ─────────────────────────────────────────────────
        ("red", 50) => Some(Color::srgba_u8(0xfe, 0xf2, 0xf2, 255)),
        ("red", 100) => Some(Color::srgba_u8(0xfe, 0xe2, 0xe2, 255)),
        ("red", 200) => Some(Color::srgba_u8(0xfe, 0xca, 0xca, 255)),
        ("red", 300) => Some(Color::srgba_u8(0xfc, 0xa5, 0xa5, 255)),
        ("red", 400) => Some(Color::srgba_u8(0xf8, 0x71, 0x71, 255)),
        ("red", 500) => Some(Color::srgba_u8(0xef, 0x44, 0x44, 255)),
        ("red", 600) => Some(Color::srgba_u8(0xdc, 0x26, 0x26, 255)),
        ("red", 700) => Some(Color::srgba_u8(0xb9, 0x1c, 0x1c, 255)),
        ("red", 800) => Some(Color::srgba_u8(0x99, 0x1b, 0x1b, 255)),
        ("red", 900) => Some(Color::srgba_u8(0x7f, 0x1d, 0x1d, 255)),
        ("red", 950) => Some(Color::srgba_u8(0x45, 0x0a, 0x0a, 255)),

        // ── orange ──────────────────────────────────────────────
        ("orange", 50) => Some(Color::srgba_u8(0xff, 0xf7, 0xed, 255)),
        ("orange", 100) => Some(Color::srgba_u8(0xff, 0xed, 0xd5, 255)),
        ("orange", 200) => Some(Color::srgba_u8(0xfe, 0xd7, 0xaa, 255)),
        ("orange", 300) => Some(Color::srgba_u8(0xfd, 0xba, 0x74, 255)),
        ("orange", 400) => Some(Color::srgba_u8(0xfb, 0x92, 0x3c, 255)),
        ("orange", 500) => Some(Color::srgba_u8(0xf9, 0x73, 0x16, 255)),
        ("orange", 600) => Some(Color::srgba_u8(0xea, 0x58, 0x0c, 255)),
        ("orange", 700) => Some(Color::srgba_u8(0xc2, 0x41, 0x0c, 255)),
        ("orange", 800) => Some(Color::srgba_u8(0x9a, 0x34, 0x12, 255)),
        ("orange", 900) => Some(Color::srgba_u8(0x7c, 0x2d, 0x12, 255)),
        ("orange", 950) => Some(Color::srgba_u8(0x43, 0x14, 0x07, 255)),

        // ── amber ───────────────────────────────────────────────
        ("amber", 50) => Some(Color::srgba_u8(0xff, 0xfb, 0xeb, 255)),
        ("amber", 100) => Some(Color::srgba_u8(0xfe, 0xf3, 0xc7, 255)),
        ("amber", 200) => Some(Color::srgba_u8(0xfd, 0xe6, 0x8a, 255)),
        ("amber", 300) => Some(Color::srgba_u8(0xfc, 0xd3, 0x4d, 255)),
        ("amber", 400) => Some(Color::srgba_u8(0xfb, 0xbf, 0x24, 255)),
        ("amber", 500) => Some(Color::srgba_u8(0xf5, 0x9e, 0x0b, 255)),
        ("amber", 600) => Some(Color::srgba_u8(0xd9, 0x77, 0x06, 255)),
        ("amber", 700) => Some(Color::srgba_u8(0xb4, 0x53, 0x09, 255)),
        ("amber", 800) => Some(Color::srgba_u8(0x92, 0x40, 0x0e, 255)),
        ("amber", 900) => Some(Color::srgba_u8(0x78, 0x35, 0x0f, 255)),
        ("amber", 950) => Some(Color::srgba_u8(0x45, 0x1a, 0x03, 255)),

        // ── yellow ──────────────────────────────────────────────
        ("yellow", 50) => Some(Color::srgba_u8(0xfe, 0xfc, 0xe8, 255)),
        ("yellow", 100) => Some(Color::srgba_u8(0xfe, 0xf9, 0xc3, 255)),
        ("yellow", 200) => Some(Color::srgba_u8(0xfe, 0xf0, 0x8a, 255)),
        ("yellow", 300) => Some(Color::srgba_u8(0xfd, 0xe0, 0x47, 255)),
        ("yellow", 400) => Some(Color::srgba_u8(0xfa, 0xcc, 0x15, 255)),
        ("yellow", 500) => Some(Color::srgba_u8(0xea, 0xb3, 0x08, 255)),
        ("yellow", 600) => Some(Color::srgba_u8(0xca, 0x8a, 0x04, 255)),
        ("yellow", 700) => Some(Color::srgba_u8(0xa1, 0x62, 0x07, 255)),
        ("yellow", 800) => Some(Color::srgba_u8(0x85, 0x4d, 0x0e, 255)),
        ("yellow", 900) => Some(Color::srgba_u8(0x71, 0x3f, 0x12, 255)),
        ("yellow", 950) => Some(Color::srgba_u8(0x42, 0x20, 0x06, 255)),

        // ── lime ────────────────────────────────────────────────
        ("lime", 50) => Some(Color::srgba_u8(0xf7, 0xfe, 0xe7, 255)),
        ("lime", 100) => Some(Color::srgba_u8(0xec, 0xfc, 0xcb, 255)),
        ("lime", 200) => Some(Color::srgba_u8(0xd9, 0xf9, 0x9d, 255)),
        ("lime", 300) => Some(Color::srgba_u8(0xbe, 0xf2, 0x64, 255)),
        ("lime", 400) => Some(Color::srgba_u8(0xa3, 0xe6, 0x35, 255)),
        ("lime", 500) => Some(Color::srgba_u8(0x84, 0xcc, 0x16, 255)),
        ("lime", 600) => Some(Color::srgba_u8(0x65, 0xa3, 0x0d, 255)),
        ("lime", 700) => Some(Color::srgba_u8(0x4d, 0x7c, 0x0f, 255)),
        ("lime", 800) => Some(Color::srgba_u8(0x3f, 0x62, 0x12, 255)),
        ("lime", 900) => Some(Color::srgba_u8(0x36, 0x53, 0x14, 255)),
        ("lime", 950) => Some(Color::srgba_u8(0x1a, 0x2e, 0x05, 255)),

        // ── green ───────────────────────────────────────────────
        ("green", 50) => Some(Color::srgba_u8(0xf0, 0xfd, 0xf4, 255)),
        ("green", 100) => Some(Color::srgba_u8(0xdc, 0xfc, 0xe7, 255)),
        ("green", 200) => Some(Color::srgba_u8(0xbb, 0xf7, 0xd0, 255)),
        ("green", 300) => Some(Color::srgba_u8(0x86, 0xef, 0xac, 255)),
        ("green", 400) => Some(Color::srgba_u8(0x4a, 0xde, 0x80, 255)),
        ("green", 500) => Some(Color::srgba_u8(0x22, 0xc5, 0x5e, 255)),
        ("green", 600) => Some(Color::srgba_u8(0x16, 0xa3, 0x4a, 255)),
        ("green", 700) => Some(Color::srgba_u8(0x15, 0x80, 0x3d, 255)),
        ("green", 800) => Some(Color::srgba_u8(0x16, 0x65, 0x34, 255)),
        ("green", 900) => Some(Color::srgba_u8(0x14, 0x53, 0x2d, 255)),
        ("green", 950) => Some(Color::srgba_u8(0x05, 0x2e, 0x16, 255)),

        // ── emerald ─────────────────────────────────────────────
        ("emerald", 50) => Some(Color::srgba_u8(0xec, 0xfd, 0xf5, 255)),
        ("emerald", 100) => Some(Color::srgba_u8(0xd1, 0xfa, 0xe5, 255)),
        ("emerald", 200) => Some(Color::srgba_u8(0xa7, 0xf3, 0xd0, 255)),
        ("emerald", 300) => Some(Color::srgba_u8(0x6e, 0xe7, 0xb7, 255)),
        ("emerald", 400) => Some(Color::srgba_u8(0x34, 0xd3, 0x99, 255)),
        ("emerald", 500) => Some(Color::srgba_u8(0x10, 0xb9, 0x81, 255)),
        ("emerald", 600) => Some(Color::srgba_u8(0x05, 0x96, 0x69, 255)),
        ("emerald", 700) => Some(Color::srgba_u8(0x04, 0x78, 0x57, 255)),
        ("emerald", 800) => Some(Color::srgba_u8(0x06, 0x5f, 0x46, 255)),
        ("emerald", 900) => Some(Color::srgba_u8(0x06, 0x4e, 0x3b, 255)),
        ("emerald", 950) => Some(Color::srgba_u8(0x02, 0x2c, 0x22, 255)),

        // ── teal ────────────────────────────────────────────────
        ("teal", 50) => Some(Color::srgba_u8(0xf0, 0xfd, 0xfa, 255)),
        ("teal", 100) => Some(Color::srgba_u8(0xcc, 0xfb, 0xf1, 255)),
        ("teal", 200) => Some(Color::srgba_u8(0x99, 0xf6, 0xe4, 255)),
        ("teal", 300) => Some(Color::srgba_u8(0x5e, 0xea, 0xd4, 255)),
        ("teal", 400) => Some(Color::srgba_u8(0x2d, 0xd4, 0xbf, 255)),
        ("teal", 500) => Some(Color::srgba_u8(0x14, 0xb8, 0xa6, 255)),
        ("teal", 600) => Some(Color::srgba_u8(0x0d, 0x94, 0x88, 255)),
        ("teal", 700) => Some(Color::srgba_u8(0x0f, 0x76, 0x6e, 255)),
        ("teal", 800) => Some(Color::srgba_u8(0x11, 0x5e, 0x59, 255)),
        ("teal", 900) => Some(Color::srgba_u8(0x13, 0x4e, 0x4a, 255)),
        ("teal", 950) => Some(Color::srgba_u8(0x04, 0x2f, 0x2e, 255)),

        // ── cyan ────────────────────────────────────────────────
        ("cyan", 50) => Some(Color::srgba_u8(0xec, 0xfe, 0xff, 255)),
        ("cyan", 100) => Some(Color::srgba_u8(0xcf, 0xfa, 0xfe, 255)),
        ("cyan", 200) => Some(Color::srgba_u8(0xa5, 0xf3, 0xfc, 255)),
        ("cyan", 300) => Some(Color::srgba_u8(0x67, 0xe8, 0xf9, 255)),
        ("cyan", 400) => Some(Color::srgba_u8(0x22, 0xd3, 0xee, 255)),
        ("cyan", 500) => Some(Color::srgba_u8(0x06, 0xb6, 0xd4, 255)),
        ("cyan", 600) => Some(Color::srgba_u8(0x08, 0x91, 0xb2, 255)),
        ("cyan", 700) => Some(Color::srgba_u8(0x0e, 0x74, 0x90, 255)),
        ("cyan", 800) => Some(Color::srgba_u8(0x15, 0x5e, 0x75, 255)),
        ("cyan", 900) => Some(Color::srgba_u8(0x16, 0x4e, 0x63, 255)),
        ("cyan", 950) => Some(Color::srgba_u8(0x08, 0x33, 0x44, 255)),

        // ── sky ─────────────────────────────────────────────────
        ("sky", 50) => Some(Color::srgba_u8(0xf0, 0xf9, 0xff, 255)),
        ("sky", 100) => Some(Color::srgba_u8(0xe0, 0xf2, 0xfe, 255)),
        ("sky", 200) => Some(Color::srgba_u8(0xba, 0xe6, 0xfd, 255)),
        ("sky", 300) => Some(Color::srgba_u8(0x7d, 0xd3, 0xfc, 255)),
        ("sky", 400) => Some(Color::srgba_u8(0x38, 0xbd, 0xf8, 255)),
        ("sky", 500) => Some(Color::srgba_u8(0x0e, 0xa5, 0xe9, 255)),
        ("sky", 600) => Some(Color::srgba_u8(0x02, 0x84, 0xc7, 255)),
        ("sky", 700) => Some(Color::srgba_u8(0x03, 0x69, 0xa1, 255)),
        ("sky", 800) => Some(Color::srgba_u8(0x07, 0x59, 0x85, 255)),
        ("sky", 900) => Some(Color::srgba_u8(0x0c, 0x4a, 0x6e, 255)),
        ("sky", 950) => Some(Color::srgba_u8(0x08, 0x2f, 0x49, 255)),

        // ── blue ────────────────────────────────────────────────
        ("blue", 50) => Some(Color::srgba_u8(0xef, 0xf6, 0xff, 255)),
        ("blue", 100) => Some(Color::srgba_u8(0xdb, 0xea, 0xfe, 255)),
        ("blue", 200) => Some(Color::srgba_u8(0xbf, 0xdb, 0xfe, 255)),
        ("blue", 300) => Some(Color::srgba_u8(0x93, 0xc5, 0xfd, 255)),
        ("blue", 400) => Some(Color::srgba_u8(0x60, 0xa5, 0xfa, 255)),
        ("blue", 500) => Some(Color::srgba_u8(0x3b, 0x82, 0xf6, 255)),
        ("blue", 600) => Some(Color::srgba_u8(0x25, 0x63, 0xeb, 255)),
        ("blue", 700) => Some(Color::srgba_u8(0x1d, 0x4e, 0xd8, 255)),
        ("blue", 800) => Some(Color::srgba_u8(0x1e, 0x40, 0xaf, 255)),
        ("blue", 900) => Some(Color::srgba_u8(0x1e, 0x3a, 0x8a, 255)),
        ("blue", 950) => Some(Color::srgba_u8(0x17, 0x25, 0x54, 255)),

        // ── indigo ──────────────────────────────────────────────
        ("indigo", 50) => Some(Color::srgba_u8(0xee, 0xf2, 0xff, 255)),
        ("indigo", 100) => Some(Color::srgba_u8(0xe0, 0xe7, 0xff, 255)),
        ("indigo", 200) => Some(Color::srgba_u8(0xc7, 0xd2, 0xfe, 255)),
        ("indigo", 300) => Some(Color::srgba_u8(0xa5, 0xb4, 0xfc, 255)),
        ("indigo", 400) => Some(Color::srgba_u8(0x81, 0x8c, 0xf8, 255)),
        ("indigo", 500) => Some(Color::srgba_u8(0x63, 0x66, 0xf1, 255)),
        ("indigo", 600) => Some(Color::srgba_u8(0x4f, 0x46, 0xe5, 255)),
        ("indigo", 700) => Some(Color::srgba_u8(0x43, 0x38, 0xca, 255)),
        ("indigo", 800) => Some(Color::srgba_u8(0x37, 0x30, 0xa3, 255)),
        ("indigo", 900) => Some(Color::srgba_u8(0x31, 0x2e, 0x81, 255)),
        ("indigo", 950) => Some(Color::srgba_u8(0x1e, 0x1b, 0x4b, 255)),

        // ── violet ──────────────────────────────────────────────
        ("violet", 50) => Some(Color::srgba_u8(0xf5, 0xf3, 0xff, 255)),
        ("violet", 100) => Some(Color::srgba_u8(0xed, 0xe9, 0xfe, 255)),
        ("violet", 200) => Some(Color::srgba_u8(0xdd, 0xd6, 0xfe, 255)),
        ("violet", 300) => Some(Color::srgba_u8(0xc4, 0xb5, 0xfd, 255)),
        ("violet", 400) => Some(Color::srgba_u8(0xa7, 0x8b, 0xfa, 255)),
        ("violet", 500) => Some(Color::srgba_u8(0x8b, 0x5c, 0xf6, 255)),
        ("violet", 600) => Some(Color::srgba_u8(0x7c, 0x3a, 0xed, 255)),
        ("violet", 700) => Some(Color::srgba_u8(0x6d, 0x28, 0xd9, 255)),
        ("violet", 800) => Some(Color::srgba_u8(0x5b, 0x21, 0xb6, 255)),
        ("violet", 900) => Some(Color::srgba_u8(0x4c, 0x1d, 0x95, 255)),
        ("violet", 950) => Some(Color::srgba_u8(0x2e, 0x10, 0x65, 255)),

        // ── purple ──────────────────────────────────────────────
        ("purple", 50) => Some(Color::srgba_u8(0xfa, 0xf5, 0xff, 255)),
        ("purple", 100) => Some(Color::srgba_u8(0xf3, 0xe8, 0xff, 255)),
        ("purple", 200) => Some(Color::srgba_u8(0xe9, 0xd5, 0xff, 255)),
        ("purple", 300) => Some(Color::srgba_u8(0xd8, 0xb4, 0xfe, 255)),
        ("purple", 400) => Some(Color::srgba_u8(0xc0, 0x84, 0xfc, 255)),
        ("purple", 500) => Some(Color::srgba_u8(0xa8, 0x55, 0xf7, 255)),
        ("purple", 600) => Some(Color::srgba_u8(0x93, 0x33, 0xea, 255)),
        ("purple", 700) => Some(Color::srgba_u8(0x7e, 0x22, 0xce, 255)),
        ("purple", 800) => Some(Color::srgba_u8(0x6b, 0x21, 0xa8, 255)),
        ("purple", 900) => Some(Color::srgba_u8(0x58, 0x1c, 0x87, 255)),
        ("purple", 950) => Some(Color::srgba_u8(0x3b, 0x07, 0x64, 255)),

        // ── fuchsia ─────────────────────────────────────────────
        ("fuchsia", 50) => Some(Color::srgba_u8(0xfd, 0xf4, 0xff, 255)),
        ("fuchsia", 100) => Some(Color::srgba_u8(0xfa, 0xe8, 0xff, 255)),
        ("fuchsia", 200) => Some(Color::srgba_u8(0xf5, 0xd0, 0xfe, 255)),
        ("fuchsia", 300) => Some(Color::srgba_u8(0xf0, 0xab, 0xfc, 255)),
        ("fuchsia", 400) => Some(Color::srgba_u8(0xe8, 0x79, 0xf9, 255)),
        ("fuchsia", 500) => Some(Color::srgba_u8(0xd9, 0x46, 0xef, 255)),
        ("fuchsia", 600) => Some(Color::srgba_u8(0xc0, 0x26, 0xd3, 255)),
        ("fuchsia", 700) => Some(Color::srgba_u8(0xa2, 0x1c, 0xaf, 255)),
        ("fuchsia", 800) => Some(Color::srgba_u8(0x86, 0x19, 0x8f, 255)),
        ("fuchsia", 900) => Some(Color::srgba_u8(0x70, 0x1a, 0x75, 255)),
        ("fuchsia", 950) => Some(Color::srgba_u8(0x4a, 0x04, 0x4e, 255)),

        // ── pink ────────────────────────────────────────────────
        ("pink", 50) => Some(Color::srgba_u8(0xfd, 0xf2, 0xf8, 255)),
        ("pink", 100) => Some(Color::srgba_u8(0xfc, 0xe7, 0xf3, 255)),
        ("pink", 200) => Some(Color::srgba_u8(0xfb, 0xcf, 0xe8, 255)),
        ("pink", 300) => Some(Color::srgba_u8(0xf9, 0xa8, 0xd4, 255)),
        ("pink", 400) => Some(Color::srgba_u8(0xf4, 0x72, 0xb6, 255)),
        ("pink", 500) => Some(Color::srgba_u8(0xec, 0x48, 0x99, 255)),
        ("pink", 600) => Some(Color::srgba_u8(0xdb, 0x27, 0x77, 255)),
        ("pink", 700) => Some(Color::srgba_u8(0xbe, 0x18, 0x5d, 255)),
        ("pink", 800) => Some(Color::srgba_u8(0x9d, 0x17, 0x4d, 255)),
        ("pink", 900) => Some(Color::srgba_u8(0x83, 0x18, 0x43, 255)),
        ("pink", 950) => Some(Color::srgba_u8(0x50, 0x07, 0x24, 255)),

        // ── rose ────────────────────────────────────────────────
        ("rose", 50) => Some(Color::srgba_u8(0xff, 0xf1, 0xf2, 255)),
        ("rose", 100) => Some(Color::srgba_u8(0xff, 0xe4, 0xe6, 255)),
        ("rose", 200) => Some(Color::srgba_u8(0xfe, 0xcd, 0xd3, 255)),
        ("rose", 300) => Some(Color::srgba_u8(0xfd, 0xa4, 0xaf, 255)),
        ("rose", 400) => Some(Color::srgba_u8(0xfb, 0x71, 0x85, 255)),
        ("rose", 500) => Some(Color::srgba_u8(0xf4, 0x3f, 0x5e, 255)),
        ("rose", 600) => Some(Color::srgba_u8(0xe1, 0x1d, 0x48, 255)),
        ("rose", 700) => Some(Color::srgba_u8(0xbe, 0x12, 0x3c, 255)),
        ("rose", 800) => Some(Color::srgba_u8(0x9f, 0x12, 0x39, 255)),
        ("rose", 900) => Some(Color::srgba_u8(0x88, 0x13, 0x37, 255)),
        ("rose", 950) => Some(Color::srgba_u8(0x4c, 0x05, 0x19, 255)),

        _ => None,
    }
}
