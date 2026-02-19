//! TTY terminal emulation state and systems.
//!
//! Uses `alacritty_terminal` to process VT100/ANSI byte streams and renders
//! the terminal grid as Bevy Text entities (one per row).

use bevy::prelude::*;
use bevy::window::RequestRedraw;

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::Color as AnsiColor;

use crate::components::ByoTty;
use crate::id_map::IdMap;
use crate::io::TtyInput;
use crate::props::tty::TtyProps;

/// Terminal dimensions for `alacritty_terminal`.
struct TermSize {
    cols: usize,
    lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

// ---------------------------------------------------------------------------
// Render cache — keeps entity IDs so reconcile can update in place.
// ---------------------------------------------------------------------------

/// Cached state for a single styled run (cell container + text child).
struct CachedRun {
    cell_entity: Entity,
    text_entity: Entity,
    text: String,
    fg: AnsiColor,
    bg: AnsiColor,
}

/// Cached state for a rendered row.
struct CachedRow {
    entity: Entity,
    runs: Vec<CachedRun>,
}

/// A freshly-built run with pre-resolved Bevy colors.
struct NewRun {
    text: String,
    fg_ansi: AnsiColor,
    bg_ansi: AnsiColor,
    fg_color: Color,
    bg_color: Color,
}

// ---------------------------------------------------------------------------
// TtyState component
// ---------------------------------------------------------------------------

/// VTE processor + terminal state, stored as a Bevy component.
#[derive(Component)]
pub struct TtyState {
    term: Term<VoidListener>,
    processor: alacritty_terminal::vte::ansi::Processor,
    cols: usize,
    rows: usize,
    dirty: bool,
    /// Cached row/run entities for incremental reconciliation.
    rendered: Vec<CachedRow>,
    /// Line height used for the cached rows (triggers rebuild if changed).
    rendered_line_h: f32,
    /// Set by resize() to force a full rebuild on next reconcile.
    force_rebuild: bool,
}

impl TtyState {
    /// Create a new terminal state with the given dimensions and scrollback.
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let size = TermSize { cols, lines: rows };
        let config = TermConfig {
            scrolling_history: scrollback,
            ..TermConfig::default()
        };
        let term = Term::new(config, &size, VoidListener);
        Self {
            term,
            processor: alacritty_terminal::vte::ansi::Processor::new(),
            cols,
            rows,
            dirty: true, // render on first frame
            rendered: Vec::new(),
            rendered_line_h: 0.0,
            force_rebuild: false,
        }
    }

    /// Feed raw bytes into the terminal emulator.
    ///
    /// Translates lone `\n` (LF) into `\r\n` (CR+LF) to mimic the kernel tty
    /// driver's `onlcr` behaviour, since passthrough bypasses the real tty.
    pub fn feed(&mut self, data: &[u8]) {
        // Fast path: no newlines at all.
        if !data.contains(&b'\n') {
            self.processor.advance(&mut self.term, data);
        } else {
            let mut start = 0;
            for i in 0..data.len() {
                if data[i] == b'\n' {
                    // Feed everything up to (not including) the LF.
                    if i > start {
                        self.processor.advance(&mut self.term, &data[start..i]);
                    }
                    // Insert CR+LF.
                    self.processor.advance(&mut self.term, b"\r\n");
                    start = i + 1;
                }
            }
            // Feed any remaining bytes after the last LF.
            if start < data.len() {
                self.processor.advance(&mut self.term, &data[start..]);
            }
        }
        self.dirty = true;
    }

    /// Resize the terminal grid.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols != self.cols || rows != self.rows {
            self.cols = cols;
            self.rows = rows;
            let size = TermSize { cols, lines: rows };
            self.term.resize(size);
            self.dirty = true;
            self.force_rebuild = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// PreUpdate system: reads `TtyInput` messages and feeds data to target TTY entities.
pub fn feed_tty_input(
    mut messages: MessageReader<TtyInput>,
    id_map: Res<IdMap>,
    mut ttys: Query<&mut TtyState>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut fed_any = false;
    for input in messages.read() {
        let entity = if input.target == "/" {
            id_map.get_entity("/")
        } else {
            id_map.get_entity(&input.target)
        };
        if let Some(entity) = entity
            && let Ok(mut state) = ttys.get_mut(entity)
        {
            state.feed(&input.data);
            fed_any = true;
        }
    }
    if fed_any {
        redraw.write(RequestRedraw);
    }
}

/// PostUpdate system: calculates terminal grid size from layout dimensions.
///
/// If the tty has fixed `cols`/`rows` (via wire prop or class), those are used directly.
/// Otherwise, dimensions are auto-calculated from the container's computed size and font metrics.
pub fn resize_tty(
    mut ttys: Query<(&ComputedNode, &TtyProps, &mut TtyState), Changed<ComputedNode>>,
) {
    for (computed, props, mut state) in &mut ttys {
        let resolved = crate::style::resolve_tty_props(props);

        let cols = if let Some(c) = resolved.cols {
            c as usize
        } else {
            let sf = computed.inverse_scale_factor;
            let logical_w = computed.size.x * sf;
            let char_w = resolved.font_size * 0.5;
            (logical_w / char_w).floor().max(1.0) as usize
        };

        let rows = if let Some(r) = resolved.rows {
            r as usize
        } else {
            let sf = computed.inverse_scale_factor;
            let logical_h = computed.size.y * sf;
            let char_h = resolved.font_size * 1.2;
            (logical_h / char_h).floor().max(1.0) as usize
        };

        state.resize(cols, rows);
    }
}

/// PostUpdate system: incrementally reconciles text children for dirty TTY entities.
///
/// Reuses existing row/run entities when possible — only updates `Text`, `TextColor`,
/// and `BackgroundColor` in place. Spawns/despawns entities only when the row count
/// or run structure within a row actually changes.
#[allow(clippy::too_many_arguments)]
pub fn reconcile_tty(
    mut commands: Commands,
    mut ttys: Query<(Entity, &mut TtyState, &TtyProps), With<ByoTty>>,
    mut texts: Query<&mut Text, Without<ByoTty>>,
    mut text_colors: Query<&mut TextColor, Without<ByoTty>>,
    mut bg_colors: Query<&mut BackgroundColor, Without<ByoTty>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let default_bg = AnsiColor::Named(alacritty_terminal::vte::ansi::NamedColor::Background);

    for (entity, mut state, props) in &mut ttys {
        if !state.dirty {
            continue;
        }
        state.dirty = false;

        let resolved = crate::style::resolve_tty_props(props);
        let font_size = resolved.font_size;
        let line_h = font_size * 1.2;

        // Check if a full rebuild is needed (resize or font-size change).
        let need_rebuild = state.force_rebuild || (line_h - state.rendered_line_h).abs() > 0.01;
        if need_rebuild {
            for row in state.rendered.drain(..) {
                commands.entity(row.entity).despawn();
            }
            state.force_rebuild = false;
        }
        state.rendered_line_h = line_h;

        // Phase 1: build new row/run data from the terminal grid.
        // This borrows `state.term` via `renderable_content()`, so we collect
        // everything into owned data before touching `state.rendered`.
        let new_rows = {
            let content = state.term.renderable_content();
            let colors = content.colors;
            let num_cols = state.cols;
            let num_rows = state.rows;

            // Collect cells into rows.
            let mut rows: Vec<Vec<(char, AnsiColor, AnsiColor)>> = vec![
                vec![
                    (
                        ' ',
                        AnsiColor::Named(alacritty_terminal::vte::ansi::NamedColor::Foreground),
                        default_bg,
                    );
                    num_cols
                ];
                num_rows
            ];

            for cell in content.display_iter {
                let point = cell.point;
                let row = point.line.0 as usize;
                let col = point.column.0;
                if row < num_rows && col < num_cols {
                    let flags = cell.flags;
                    if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                        continue;
                    }
                    let (fg, bg) = if flags.contains(CellFlags::INVERSE) {
                        (cell.bg, cell.fg)
                    } else {
                        (cell.fg, cell.bg)
                    };
                    rows[row][col] = (cell.c, fg, bg);
                }
            }

            // Build runs per row with pre-resolved Bevy colors.
            build_all_runs(&rows, colors, &default_bg)
        };
        // `content` is dropped — `state.term` borrow released.

        // Phase 2: diff new_rows against the cache and update in place.
        let old_count = state.rendered.len();
        let new_count = new_rows.len();

        // Update/reuse existing rows.
        for i in 0..old_count.min(new_count) {
            let new_runs = &new_rows[i];

            if state.rendered[i].runs.len() == new_runs.len() {
                // Same run structure — update components in place.
                for j in 0..new_runs.len() {
                    let text_entity = state.rendered[i].runs[j].text_entity;
                    let cell_entity = state.rendered[i].runs[j].cell_entity;
                    let new_r = &new_runs[j];

                    if state.rendered[i].runs[j].text != new_r.text {
                        if let Ok(mut t) = texts.get_mut(text_entity) {
                            t.0.clone_from(&new_r.text);
                        }
                        state.rendered[i].runs[j].text.clone_from(&new_r.text);
                    }
                    if state.rendered[i].runs[j].fg != new_r.fg_ansi {
                        if let Ok(mut tc) = text_colors.get_mut(text_entity) {
                            *tc = TextColor(new_r.fg_color);
                        }
                        state.rendered[i].runs[j].fg = new_r.fg_ansi;
                    }
                    if state.rendered[i].runs[j].bg != new_r.bg_ansi {
                        if let Ok(mut bc) = bg_colors.get_mut(cell_entity) {
                            *bc = BackgroundColor(new_r.bg_color);
                        }
                        state.rendered[i].runs[j].bg = new_r.bg_ansi;
                    }
                }
            } else {
                // Different run count — rebuild this row's children.
                let row_entity = state.rendered[i].entity;
                for old_run in &state.rendered[i].runs {
                    commands.entity(old_run.cell_entity).despawn();
                }
                state.rendered[i].runs =
                    spawn_runs(&mut commands, row_entity, new_runs, font_size, line_h);
            }
        }

        // Spawn new rows beyond old count.
        for new_runs in new_rows.iter().skip(old_count) {
            let cached = spawn_full_row(&mut commands, entity, new_runs, font_size, line_h);
            state.rendered.push(cached);
        }

        // Despawn excess old rows.
        for row in state.rendered.drain(new_count..) {
            commands.entity(row.entity).despawn();
        }

        redraw.write(RequestRedraw);
    }
}

// ---------------------------------------------------------------------------
// Run building helpers
// ---------------------------------------------------------------------------

/// Build runs for all rows, pre-resolving AnsiColor → Bevy Color.
fn build_all_runs(
    rows: &[Vec<(char, AnsiColor, AnsiColor)>],
    colors: &alacritty_terminal::term::color::Colors,
    default_bg: &AnsiColor,
) -> Vec<Vec<NewRun>> {
    rows.iter()
        .map(|row| build_row_runs(row, colors, default_bg))
        .collect()
}

/// Group consecutive same-styled cells into runs, trimming trailing blanks.
fn build_row_runs(
    row: &[(char, AnsiColor, AnsiColor)],
    colors: &alacritty_terminal::term::color::Colors,
    default_bg: &AnsiColor,
) -> Vec<NewRun> {
    let mut runs = Vec::new();
    let mut start = 0;

    while start < row.len() {
        let (_, fg, bg) = row[start];
        let mut end = start + 1;
        while end < row.len() {
            let (_, fg2, bg2) = row[end];
            if fg == fg2 && bg == bg2 {
                end += 1;
            } else {
                break;
            }
        }
        let text: String = row[start..end].iter().map(|(c, ..)| c).collect();
        let fg_color = ansi_color_to_bevy(fg, colors);
        let bg_color = if bg != *default_bg {
            ansi_color_to_bevy(bg, colors)
        } else {
            Color::NONE
        };
        runs.push(NewRun {
            text,
            fg_ansi: fg,
            bg_ansi: bg,
            fg_color,
            bg_color,
        });
        start = end;
    }

    // Trim trailing whitespace-only runs with default styling.
    while let Some(run) = runs.last() {
        if run.bg_ansi == *default_bg && run.text.chars().all(|c| c == ' ') {
            runs.pop();
        } else {
            break;
        }
    }

    runs
}

// ---------------------------------------------------------------------------
// Entity spawning helpers
// ---------------------------------------------------------------------------

/// Spawn a complete row entity with its run children. Returns the cache entry.
fn spawn_full_row(
    commands: &mut Commands,
    tty_entity: Entity,
    runs: &[NewRun],
    font_size: f32,
    line_h: f32,
) -> CachedRow {
    let row_entity = commands
        .spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::NoWrap,
                height: Val::Px(line_h),
                align_self: AlignSelf::FlexStart,
                overflow: Overflow::clip(),
                ..default()
            },
            ChildOf(tty_entity),
        ))
        .id();

    let cached_runs = spawn_runs(commands, row_entity, runs, font_size, line_h);
    CachedRow {
        entity: row_entity,
        runs: cached_runs,
    }
}

/// Spawn run children (cell + text) inside an existing row entity.
fn spawn_runs(
    commands: &mut Commands,
    row_entity: Entity,
    runs: &[NewRun],
    font_size: f32,
    line_h: f32,
) -> Vec<CachedRun> {
    runs.iter()
        .map(|run| {
            let cell_entity = commands
                .spawn((
                    Node {
                        height: Val::Px(line_h),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(run.bg_color),
                    ChildOf(row_entity),
                ))
                .id();

            let text_entity = commands
                .spawn((
                    Text::new(&run.text),
                    TextFont {
                        font_size,
                        ..default()
                    },
                    TextColor(run.fg_color),
                    ChildOf(cell_entity),
                ))
                .id();

            CachedRun {
                cell_entity,
                text_entity,
                text: run.text.clone(),
                fg: run.fg_ansi,
                bg: run.bg_ansi,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Root tty setup
// ---------------------------------------------------------------------------

/// Spawn the implicit root tty entity — the default passthrough target.
/// Absolutely positioned and transparent so normal views show through it.
pub fn setup_root_tty(mut commands: Commands, mut id_map: ResMut<IdMap>) {
    let tty_props = TtyProps::default();
    let resolved = crate::style::resolve_tty_props(&tty_props);
    let state = TtyState::new(80, 24, resolved.scrollback as usize);
    let entity = commands
        .spawn((
            ByoTty,
            tty_props,
            state,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                overflow: Overflow::clip(),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                ..default()
            },
        ))
        .id();
    id_map.insert("/".to_string(), entity);
    info!("root tty spawned: {entity:?}");
}

// ---------------------------------------------------------------------------
// ANSI color conversion
// ---------------------------------------------------------------------------

/// Convert an alacritty ANSI color to a Bevy Color, using the terminal palette.
fn ansi_color_to_bevy(
    color: AnsiColor,
    palette: &alacritty_terminal::term::color::Colors,
) -> Color {
    match color {
        AnsiColor::Named(named) => {
            if let Some(rgb) = palette[named] {
                Color::srgb_u8(rgb.r, rgb.g, rgb.b)
            } else {
                default_named_color(named)
            }
        }
        AnsiColor::Spec(rgb) => Color::srgb_u8(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => {
            if let Some(rgb) = palette[idx as usize] {
                Color::srgb_u8(rgb.r, rgb.g, rgb.b)
            } else {
                default_indexed_color(idx)
            }
        }
    }
}

/// Default ANSI named colors when the palette doesn't override them.
fn default_named_color(named: alacritty_terminal::vte::ansi::NamedColor) -> Color {
    use alacritty_terminal::vte::ansi::NamedColor::*;
    match named {
        Black | DimBlack => Color::srgb_u8(0, 0, 0),
        Red => Color::srgb_u8(205, 49, 49),
        Green => Color::srgb_u8(13, 188, 121),
        Yellow => Color::srgb_u8(229, 229, 16),
        Blue => Color::srgb_u8(36, 114, 200),
        Magenta => Color::srgb_u8(188, 63, 188),
        Cyan => Color::srgb_u8(17, 168, 205),
        White => Color::srgb_u8(229, 229, 229),
        BrightBlack => Color::srgb_u8(102, 102, 102),
        BrightRed | DimRed => Color::srgb_u8(241, 76, 76),
        BrightGreen | DimGreen => Color::srgb_u8(35, 209, 139),
        BrightYellow | DimYellow => Color::srgb_u8(245, 245, 67),
        BrightBlue | DimBlue => Color::srgb_u8(59, 142, 234),
        BrightMagenta | DimMagenta => Color::srgb_u8(214, 112, 214),
        BrightCyan | DimCyan => Color::srgb_u8(41, 184, 219),
        BrightWhite | DimWhite => Color::srgb_u8(255, 255, 255),
        Foreground | BrightForeground | DimForeground => Color::srgb_u8(229, 229, 229),
        Background => Color::srgb_u8(0, 0, 0),
        Cursor => Color::srgb_u8(229, 229, 229),
    }
}

/// Default 256-color palette for indexed colors.
fn default_indexed_color(idx: u8) -> Color {
    match idx {
        // Standard colors (0-15) handled by Named, but may arrive here
        0 => Color::srgb_u8(0, 0, 0),
        1 => Color::srgb_u8(205, 49, 49),
        2 => Color::srgb_u8(13, 188, 121),
        3 => Color::srgb_u8(229, 229, 16),
        4 => Color::srgb_u8(36, 114, 200),
        5 => Color::srgb_u8(188, 63, 188),
        6 => Color::srgb_u8(17, 168, 205),
        7 => Color::srgb_u8(229, 229, 229),
        8 => Color::srgb_u8(102, 102, 102),
        9 => Color::srgb_u8(241, 76, 76),
        10 => Color::srgb_u8(35, 209, 139),
        11 => Color::srgb_u8(245, 245, 67),
        12 => Color::srgb_u8(59, 142, 234),
        13 => Color::srgb_u8(214, 112, 214),
        14 => Color::srgb_u8(41, 184, 219),
        15 => Color::srgb_u8(255, 255, 255),
        // 216-color cube (16-231)
        16..=231 => {
            let n = idx - 16;
            let r = (n / 36) % 6;
            let g = (n / 6) % 6;
            let b = n % 6;
            let to_val = |c: u8| if c == 0 { 0u8 } else { 55 + 40 * c };
            Color::srgb_u8(to_val(r), to_val(g), to_val(b))
        }
        // Grayscale (232-255)
        232..=255 => {
            let v = 8 + 10 * (idx - 232);
            Color::srgb_u8(v, v, v)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::NamedColor;

    const FG: AnsiColor = AnsiColor::Named(NamedColor::Foreground);
    const BG: AnsiColor = AnsiColor::Named(NamedColor::Background);
    const RED: AnsiColor = AnsiColor::Named(NamedColor::Red);
    const GREEN: AnsiColor = AnsiColor::Named(NamedColor::Green);
    const BLUE: AnsiColor = AnsiColor::Named(NamedColor::Blue);

    fn make_state(cols: usize, rows: usize) -> TtyState {
        TtyState::new(cols, rows, 100)
    }

    /// Read back the grid as one string per row, trailing spaces trimmed.
    fn grid_text(state: &TtyState) -> Vec<String> {
        let content = state.term.renderable_content();
        let mut rows = vec![vec![' '; state.cols]; state.rows];
        for cell in content.display_iter {
            let r = cell.point.line.0 as usize;
            let c = cell.point.column.0;
            if r < state.rows && c < state.cols {
                rows[r][c] = cell.c;
            }
        }
        rows.iter()
            .map(|r| r.iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    /// Read back grid cells with fg/bg colors (applies inverse, skips spacers).
    fn grid_cells(state: &TtyState) -> Vec<Vec<(char, AnsiColor, AnsiColor)>> {
        let content = state.term.renderable_content();
        let mut rows = vec![vec![(' ', FG, BG); state.cols]; state.rows];
        for cell in content.display_iter {
            let r = cell.point.line.0 as usize;
            let c = cell.point.column.0;
            if r < state.rows && c < state.cols {
                let flags = cell.flags;
                if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let (fg, bg) = if flags.contains(CellFlags::INVERSE) {
                    (cell.bg, cell.fg)
                } else {
                    (cell.fg, cell.bg)
                };
                rows[r][c] = (cell.c, fg, bg);
            }
        }
        rows
    }

    /// Build runs from a manually-constructed row using a fresh terminal palette.
    fn test_runs(row: &[(char, AnsiColor, AnsiColor)]) -> Vec<NewRun> {
        let state = make_state(10, 1);
        let content = state.term.renderable_content();
        build_row_runs(row, content.colors, &BG)
    }

    // -----------------------------------------------------------------------
    // feed() tests
    // -----------------------------------------------------------------------

    #[test]
    fn feed_simple_text() {
        let mut state = make_state(80, 24);
        state.feed(b"hello");
        let rows = grid_text(&state);
        assert_eq!(rows[0], "hello");
    }

    #[test]
    fn feed_newline_crlf() {
        let mut state = make_state(80, 24);
        state.feed(b"a\nb");
        let rows = grid_text(&state);
        assert_eq!(rows[0], "a");
        assert_eq!(rows[1], "b");
    }

    #[test]
    fn feed_no_newline_fast_path() {
        let mut state = make_state(80, 24);
        state.feed(b"abcdef");
        let rows = grid_text(&state);
        assert_eq!(rows[0], "abcdef");
        // No content on row 1
        assert_eq!(rows[1], "");
    }

    #[test]
    fn feed_ansi_color() {
        let mut state = make_state(80, 24);
        // ESC[31m = set fg red, ESC[0m = reset
        state.feed(b"\x1b[31mred\x1b[0m");
        let cells = grid_cells(&state);
        // First three cells should have red foreground
        assert_eq!(cells[0][0].0, 'r');
        assert_eq!(cells[0][0].1, RED);
        assert_eq!(cells[0][1].0, 'e');
        assert_eq!(cells[0][1].1, RED);
        assert_eq!(cells[0][2].0, 'd');
        assert_eq!(cells[0][2].1, RED);
    }

    #[test]
    fn feed_column_wrap() {
        let mut state = make_state(5, 3);
        state.feed(b"abcdefgh");
        let rows = grid_text(&state);
        assert_eq!(rows[0], "abcde");
        assert_eq!(rows[1], "fgh");
    }

    // -----------------------------------------------------------------------
    // resize() tests
    // -----------------------------------------------------------------------

    #[test]
    fn resize_shrink() {
        let mut state = make_state(80, 24);
        state.dirty = false;
        state.resize(40, 12);
        assert_eq!(state.cols, 40);
        assert_eq!(state.rows, 12);
        assert!(state.dirty);
        assert!(state.force_rebuild);
    }

    #[test]
    fn resize_noop() {
        let mut state = make_state(80, 24);
        state.dirty = false;
        state.force_rebuild = false;
        state.resize(80, 24);
        assert!(!state.dirty, "same dimensions should not set dirty");
        assert!(!state.force_rebuild);
    }

    #[test]
    fn resize_sets_force_rebuild() {
        let mut state = make_state(80, 24);
        state.force_rebuild = false;
        state.resize(100, 30);
        assert!(state.force_rebuild);
        assert!(state.dirty);
    }

    // -----------------------------------------------------------------------
    // build_row_runs() tests
    // -----------------------------------------------------------------------

    #[test]
    fn runs_single_style() {
        let row = vec![('a', FG, BG), ('b', FG, BG), ('c', FG, BG)];
        let runs = test_runs(&row);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "abc");
        assert_eq!(runs[0].fg_ansi, FG);
        assert_eq!(runs[0].bg_ansi, BG);
    }

    #[test]
    fn runs_alternating_styles() {
        let row = vec![('r', RED, BG), ('g', GREEN, BG), ('b', BLUE, BG)];
        let runs = test_runs(&row);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text, "r");
        assert_eq!(runs[0].fg_ansi, RED);
        assert_eq!(runs[1].text, "g");
        assert_eq!(runs[1].fg_ansi, GREEN);
        assert_eq!(runs[2].text, "b");
        assert_eq!(runs[2].fg_ansi, BLUE);
    }

    #[test]
    fn runs_empty_row() {
        // All spaces with default bg → trimmed to empty
        let row = vec![(' ', FG, BG), (' ', FG, BG), (' ', FG, BG)];
        let runs = test_runs(&row);
        assert!(runs.is_empty());
    }

    #[test]
    fn runs_trailing_whitespace_trimmed() {
        let row = vec![('a', FG, BG), (' ', FG, BG), (' ', FG, BG)];
        let runs = test_runs(&row);
        // "a" run + trailing spaces with default bg → trailing run trimmed
        // The first run "a  " is one run (same style), but trailing spaces
        // within a run don't get trimmed — only trailing RUNS are trimmed.
        // Since all three cells share the same style, it's one run "a  ".
        // "a  " has trailing spaces but is not all-spaces, so it stays.
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "a  ");
    }

    #[test]
    fn runs_trailing_bg_not_trimmed() {
        // Trailing spaces with non-default bg should NOT be trimmed
        let row = vec![('a', FG, BG), (' ', FG, RED), (' ', FG, RED)];
        let runs = test_runs(&row);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "a");
        assert_eq!(runs[1].text, "  ");
        assert_eq!(runs[1].bg_ansi, RED);
    }

    #[test]
    fn runs_mixed_trim() {
        // Colored text followed by default-bg spaces: spaces trimmed
        let row = vec![('x', RED, BG), ('y', RED, BG), (' ', FG, BG), (' ', FG, BG)];
        let runs = test_runs(&row);
        // "xy" (red) + "  " (default) → trailing run trimmed
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "xy");
    }

    // -----------------------------------------------------------------------
    // Color conversion tests
    // -----------------------------------------------------------------------

    #[test]
    fn color_named_red() {
        let c = default_named_color(NamedColor::Red);
        assert_eq!(c, Color::srgb_u8(205, 49, 49));
    }

    #[test]
    fn color_named_foreground() {
        let c = default_named_color(NamedColor::Foreground);
        assert_eq!(c, Color::srgb_u8(229, 229, 229));
    }

    #[test]
    fn color_spec_rgb() {
        let state = make_state(10, 1);
        let content = state.term.renderable_content();
        let rgb = alacritty_terminal::vte::ansi::Rgb {
            r: 100,
            g: 150,
            b: 200,
        };
        let c = ansi_color_to_bevy(AnsiColor::Spec(rgb), content.colors);
        assert_eq!(c, Color::srgb_u8(100, 150, 200));
    }

    #[test]
    fn color_indexed_cube() {
        // Index 16 = first entry of 6×6×6 cube: rgb(0,0,0)
        let c = default_indexed_color(16);
        assert_eq!(c, Color::srgb_u8(0, 0, 0));
        // Index 21 = (0,0,5) → rgb(0, 0, 255)
        let c = default_indexed_color(21);
        assert_eq!(c, Color::srgb_u8(0, 0, 255));
    }

    #[test]
    fn color_indexed_grayscale() {
        // Index 232 = first grayscale: 8 + 10*(232-232) = 8
        let c = default_indexed_color(232);
        assert_eq!(c, Color::srgb_u8(8, 8, 8));
        // Index 255 = last grayscale: 8 + 10*23 = 238
        let c = default_indexed_color(255);
        assert_eq!(c, Color::srgb_u8(238, 238, 238));
    }

    // -----------------------------------------------------------------------
    // Integration: feed + grid readback
    // -----------------------------------------------------------------------

    #[test]
    fn integration_colored_text_readback() {
        let mut state = make_state(80, 24);
        // Red "hi" then green "ok"
        state.feed(b"\x1b[31mhi\x1b[32mok\x1b[0m");
        let cells = grid_cells(&state);
        assert_eq!(cells[0][0], ('h', RED, BG));
        assert_eq!(cells[0][1], ('i', RED, BG));
        assert_eq!(cells[0][2], ('o', GREEN, BG));
        assert_eq!(cells[0][3], ('k', GREEN, BG));
    }

    #[test]
    fn integration_inverse_video() {
        let mut state = make_state(80, 24);
        // ESC[7m = reverse video
        state.feed(b"\x1b[7mflip\x1b[0m");
        let cells = grid_cells(&state);
        // In inverse mode, fg and bg swap. The default fg becomes bg and vice versa.
        assert_eq!(cells[0][0].0, 'f');
        // After inverse: fg should be Background, bg should be Foreground
        assert_eq!(cells[0][0].1, BG); // fg is now the background color
        assert_eq!(cells[0][0].2, FG); // bg is now the foreground color
    }

    #[test]
    fn integration_wide_char() {
        let mut state = make_state(80, 24);
        // Feed a CJK character (full-width) followed by a normal char
        state.feed("全a".as_bytes());
        let cells = grid_cells(&state);
        // '全' occupies col 0 (the spacer at col 1 is skipped by grid_cells)
        assert_eq!(cells[0][0].0, '全');
        // col 1 is the spacer — grid_cells skips it, leaving the default
        assert_eq!(cells[0][1].0, ' ');
        // 'a' is at col 2
        assert_eq!(cells[0][2].0, 'a');
    }
}
