//! Well-known built-in type names recognized by the compositor and
//! first-party daemons.
//!
//! Third-party types use dot-qualified names (e.g. `org.mybrowser.WebView`).
//! These constants are provided for discoverability and typo prevention,
//! not as an exhaustive list — the protocol accepts any type name.

/// Types handled natively by the compositor.
pub mod compositor {
    pub const VIEW: &str = "view";
    pub const LAYER: &str = "layer";
    pub const TEXT: &str = "text";
    pub const WINDOW: &str = "window";
}

/// Types owned by the controls daemon.
pub mod controls {
    pub const BUTTON: &str = "button";
    pub const SLIDER: &str = "slider";
    pub const CHECKBOX: &str = "checkbox";
}
