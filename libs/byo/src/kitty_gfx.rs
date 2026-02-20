//! Kitty graphics protocol parser.
//!
//! Parses the control data format used by the Kitty terminal graphics protocol
//! (`ESC _ G <control>;[payload] ESC \`). The control portion is comma-separated
//! `key=value` pairs; the payload (after `;`) is base64-encoded image data.
//!
//! This module only handles parsing — decoding, storage, and rendering are
//! handled by the compositor's kitty_gfx module.

use std::fmt;

/// A parsed kitty graphics protocol command.
#[derive(Debug, Clone)]
pub struct KittyGfxCommand {
    /// Action to perform (`a=`). Default: TransmitDisplay.
    pub action: Action,
    /// Quiet mode (`q=`). 0=normal, 1=suppress OK, 2=suppress all.
    pub quiet: u8,
    /// Image ID (`i=`).
    pub image_id: Option<u32>,
    /// Image number (`I=`).
    pub image_number: Option<u32>,
    /// Placement ID (`p=`).
    pub placement_id: Option<u32>,
    /// Pixel format (`f=`). Default: Rgba.
    pub format: Format,
    /// Transmission medium (`t=`). Default: Direct.
    pub transmission: Transmission,
    /// Compression (`o=`). Default: None.
    pub compression: Compression,
    /// More chunks follow (`m=1`).
    pub more: bool,
    /// Source pixel width (`s=`).
    pub width: Option<u32>,
    /// Source pixel height (`v=`).
    pub height: Option<u32>,
    /// Display columns (`c=`).
    pub columns: Option<u32>,
    /// Display rows (`r=`).
    pub rows: Option<u32>,
    /// X pixel offset within cell (`X=`).
    pub x_offset: Option<u32>,
    /// Y pixel offset within cell (`Y=`).
    pub y_offset: Option<u32>,
    /// Z-index for stacking order (`z=`).
    pub z_index: Option<i32>,
    /// Don't move cursor after display (`C=1`).
    pub no_move_cursor: bool,
    /// Delete mode (`d=`).
    pub delete: Option<DeleteTarget>,
    /// Raw base64 payload bytes (after `;`).
    pub payload: Vec<u8>,
}

impl Default for KittyGfxCommand {
    fn default() -> Self {
        Self {
            action: Action::TransmitDisplay,
            quiet: 0,
            image_id: None,
            image_number: None,
            placement_id: None,
            format: Format::Rgba,
            transmission: Transmission::Direct,
            compression: Compression::None,
            more: false,
            width: None,
            height: None,
            columns: None,
            rows: None,
            x_offset: None,
            y_offset: None,
            z_index: None,
            no_move_cursor: false,
            delete: None,
            payload: Vec::new(),
        }
    }
}

/// Action type (`a=` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `T` or default — transmit data and display
    TransmitDisplay,
    /// `t` — transmit data only (no display)
    Transmit,
    /// `p` — display a previously transmitted image
    Put,
    /// `d` — delete images/placements
    Delete,
    /// `q` — query terminal for image support
    Query,
    /// `f` — animation frame
    Frame,
    /// `a` — animation control
    Animate,
    /// `c` — compose/blend
    Compose,
}

/// Pixel format (`f=` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `24` — RGB (3 bytes per pixel)
    Rgb,
    /// `32` — RGBA (4 bytes per pixel, default)
    Rgba,
    /// `100` — PNG compressed
    Png,
}

/// Transmission medium (`t=` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transmission {
    /// `d` — direct (payload is the data, default)
    Direct,
    /// `f` — regular file
    File,
    /// `t` — temporary file (deleted after read)
    TempFile,
    /// `s` — shared memory object
    SharedMemory,
}

/// Compression method (`o=` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// No compression (default)
    None,
    /// `z` — zlib/deflate
    Zlib,
}

/// Delete target specification (`d=` key value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteTarget {
    /// `a` or `A` — delete all images (A = free memory too)
    All { free: bool },
    /// `i` or `I` — delete by image ID (I = free memory too)
    ById {
        id: u32,
        placement: Option<u32>,
        free: bool,
    },
    /// `n` or `N` — delete by image number
    ByNumber { number: u32, free: bool },
    /// `c` or `C` — delete at cursor position
    AtCursor { free: bool },
    /// `p` or `P` — delete by placement ID
    ByPlacement { id: u32, free: bool },
    /// `z` or `Z` — delete by z-index
    ByZIndex { z: i32, free: bool },
}

/// Error type for kitty graphics parsing.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Invalid key-value pair in control data.
    InvalidControl(String),
    /// Unknown action character.
    UnknownAction(char),
    /// Unknown format value.
    UnknownFormat(u32),
    /// Unknown transmission medium.
    UnknownTransmission(char),
    /// Invalid delete specification.
    InvalidDelete(String),
    /// Invalid integer value.
    InvalidInt(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InvalidControl(s) => write!(f, "invalid control data: {s}"),
            ParseError::UnknownAction(c) => write!(f, "unknown action: {c}"),
            ParseError::UnknownFormat(n) => write!(f, "unknown format: {n}"),
            ParseError::UnknownTransmission(c) => write!(f, "unknown transmission: {c}"),
            ParseError::InvalidDelete(s) => write!(f, "invalid delete spec: {s}"),
            ParseError::InvalidInt(s) => write!(f, "invalid integer: {s}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a kitty graphics payload (bytes between `G` prefix and `ST`).
///
/// The format is: `key=value,key=value,...;base64payload`
///
/// # Examples
///
/// ```
/// use byo::kitty_gfx::{parse, Action, Format};
///
/// let cmd = parse(b"a=t,f=32,s=100,v=50,i=1;iVBORw0KGgo=").unwrap();
/// assert_eq!(cmd.action, Action::Transmit);
/// assert_eq!(cmd.format, Format::Rgba);
/// assert_eq!(cmd.width, Some(100));
/// assert_eq!(cmd.height, Some(50));
/// assert_eq!(cmd.image_id, Some(1));
/// assert_eq!(cmd.payload, b"iVBORw0KGgo=");
/// ```
pub fn parse(data: &[u8]) -> Result<KittyGfxCommand, ParseError> {
    let data_str = std::str::from_utf8(data)
        .map_err(|_| ParseError::InvalidControl("non-UTF8 control data".to_string()))?;

    // Split at first ';' — control is before, payload is after
    let (control, payload) = match data_str.find(';') {
        Some(pos) => (&data_str[..pos], data_str[pos + 1..].as_bytes()),
        None => (data_str, &[] as &[u8]),
    };

    let mut cmd = KittyGfxCommand {
        payload: payload.to_vec(),
        ..Default::default()
    };

    if control.is_empty() {
        return Ok(cmd);
    }

    // Store raw delete value for deferred resolution (needs i=, p=, z= parsed first)
    let mut raw_delete: Option<&str> = None;

    for pair in control.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }

        let Some((key, value)) = pair.split_once('=') else {
            return Err(ParseError::InvalidControl(pair.to_string()));
        };

        match key {
            "a" => {
                cmd.action = match value.as_bytes().first() {
                    Some(b'T') | None => Action::TransmitDisplay,
                    Some(b't') => Action::Transmit,
                    Some(b'p') => Action::Put,
                    Some(b'd') => Action::Delete,
                    Some(b'q') => Action::Query,
                    Some(b'f') => Action::Frame,
                    Some(b'a') => Action::Animate,
                    Some(b'c') => Action::Compose,
                    Some(&c) => return Err(ParseError::UnknownAction(c as char)),
                };
            }
            "q" => {
                cmd.quiet = parse_u32(value)? as u8;
            }
            "i" => {
                cmd.image_id = Some(parse_u32(value)?);
            }
            "I" => {
                cmd.image_number = Some(parse_u32(value)?);
            }
            "p" => {
                cmd.placement_id = Some(parse_u32(value)?);
            }
            "f" => {
                let n = parse_u32(value)?;
                cmd.format = match n {
                    24 => Format::Rgb,
                    32 => Format::Rgba,
                    100 => Format::Png,
                    _ => return Err(ParseError::UnknownFormat(n)),
                };
            }
            "t" => {
                cmd.transmission = match value.as_bytes().first() {
                    Some(b'd') | None => Transmission::Direct,
                    Some(b'f') => Transmission::File,
                    Some(b't') => Transmission::TempFile,
                    Some(b's') => Transmission::SharedMemory,
                    Some(&c) => return Err(ParseError::UnknownTransmission(c as char)),
                };
            }
            "o" => {
                cmd.compression = match value {
                    "z" => Compression::Zlib,
                    _ => Compression::None,
                };
            }
            "m" => {
                cmd.more = value == "1";
            }
            "s" => {
                cmd.width = Some(parse_u32(value)?);
            }
            "v" => {
                cmd.height = Some(parse_u32(value)?);
            }
            "c" => {
                cmd.columns = Some(parse_u32(value)?);
            }
            "r" => {
                cmd.rows = Some(parse_u32(value)?);
            }
            "X" => {
                cmd.x_offset = Some(parse_u32(value)?);
            }
            "Y" => {
                cmd.y_offset = Some(parse_u32(value)?);
            }
            "z" => {
                cmd.z_index = Some(
                    value
                        .parse::<i32>()
                        .map_err(|_| ParseError::InvalidInt(value.to_string()))?,
                );
            }
            "C" => {
                cmd.no_move_cursor = value == "1";
            }
            "d" => {
                raw_delete = Some(value);
            }
            _ => {
                // Unknown keys are silently ignored (forward compat)
            }
        }
    }

    // Resolve delete after all other keys are parsed (needs i=, p=, z=)
    if let Some(d) = raw_delete {
        cmd.delete = Some(parse_delete(d, &cmd)?);
    }

    Ok(cmd)
}

fn parse_u32(s: &str) -> Result<u32, ParseError> {
    s.parse::<u32>()
        .map_err(|_| ParseError::InvalidInt(s.to_string()))
}

fn parse_delete(value: &str, cmd: &KittyGfxCommand) -> Result<DeleteTarget, ParseError> {
    let c = value
        .as_bytes()
        .first()
        .ok_or_else(|| ParseError::InvalidDelete("empty".to_string()))?;
    let free = c.is_ascii_uppercase();
    match c.to_ascii_lowercase() {
        b'a' => Ok(DeleteTarget::All { free }),
        b'i' => Ok(DeleteTarget::ById {
            id: cmd.image_id.unwrap_or(0),
            placement: cmd.placement_id,
            free,
        }),
        b'n' => Ok(DeleteTarget::ByNumber {
            number: cmd.image_number.unwrap_or(0),
            free,
        }),
        b'c' => Ok(DeleteTarget::AtCursor { free }),
        b'p' => Ok(DeleteTarget::ByPlacement {
            id: cmd.placement_id.unwrap_or(0),
            free,
        }),
        b'z' => Ok(DeleteTarget::ByZIndex {
            z: cmd.z_index.unwrap_or(0),
            free,
        }),
        _ => Err(ParseError::InvalidDelete(value.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_transmit_display_default() {
        let cmd = parse(b"f=32,s=100,v=50;AAAA").unwrap();
        assert_eq!(cmd.action, Action::TransmitDisplay);
        assert_eq!(cmd.format, Format::Rgba);
        assert_eq!(cmd.width, Some(100));
        assert_eq!(cmd.height, Some(50));
        assert_eq!(cmd.payload, b"AAAA");
    }

    #[test]
    fn parse_transmit_only() {
        let cmd = parse(b"a=t,f=100,i=1;iVBORw0KGgo=").unwrap();
        assert_eq!(cmd.action, Action::Transmit);
        assert_eq!(cmd.format, Format::Png);
        assert_eq!(cmd.image_id, Some(1));
        assert_eq!(cmd.payload, b"iVBORw0KGgo=");
    }

    #[test]
    fn parse_put() {
        let cmd = parse(b"a=p,i=1,p=2,c=40,r=20").unwrap();
        assert_eq!(cmd.action, Action::Put);
        assert_eq!(cmd.image_id, Some(1));
        assert_eq!(cmd.placement_id, Some(2));
        assert_eq!(cmd.columns, Some(40));
        assert_eq!(cmd.rows, Some(20));
        assert!(cmd.payload.is_empty());
    }

    #[test]
    fn parse_query() {
        let cmd = parse(b"a=q,i=1,s=1,v=1,f=24;AAAA").unwrap();
        assert_eq!(cmd.action, Action::Query);
        assert_eq!(cmd.format, Format::Rgb);
    }

    #[test]
    fn parse_delete_all() {
        let cmd = parse(b"a=d,d=a").unwrap();
        assert_eq!(cmd.action, Action::Delete);
        assert_eq!(cmd.delete, Some(DeleteTarget::All { free: false }));
    }

    #[test]
    fn parse_delete_all_free() {
        let cmd = parse(b"a=d,d=A").unwrap();
        assert_eq!(cmd.delete, Some(DeleteTarget::All { free: true }));
    }

    #[test]
    fn parse_delete_by_id() {
        let cmd = parse(b"a=d,d=i,i=42").unwrap();
        assert_eq!(
            cmd.delete,
            Some(DeleteTarget::ById {
                id: 42,
                placement: None,
                free: false
            })
        );
    }

    #[test]
    fn parse_delete_by_id_with_placement() {
        let cmd = parse(b"a=d,d=I,i=42,p=3").unwrap();
        assert_eq!(
            cmd.delete,
            Some(DeleteTarget::ById {
                id: 42,
                placement: Some(3),
                free: true
            })
        );
    }

    #[test]
    fn parse_chunked_more() {
        let cmd = parse(b"a=t,f=100,i=1,m=1;chunk1data").unwrap();
        assert!(cmd.more);
        assert_eq!(cmd.payload, b"chunk1data");
    }

    #[test]
    fn parse_chunked_last() {
        let cmd = parse(b"a=t,f=100,i=1,m=0;lastchunk").unwrap();
        assert!(!cmd.more);
    }

    #[test]
    fn parse_no_move_cursor() {
        let cmd = parse(b"a=T,C=1,i=1;data").unwrap();
        assert!(cmd.no_move_cursor);
    }

    #[test]
    fn parse_zlib_compression() {
        let cmd = parse(b"a=t,o=z,f=32,s=10,v=10,i=1;compressed").unwrap();
        assert_eq!(cmd.compression, Compression::Zlib);
    }

    #[test]
    fn parse_z_index() {
        let cmd = parse(b"a=p,i=1,z=-5").unwrap();
        assert_eq!(cmd.z_index, Some(-5));
    }

    #[test]
    fn parse_unknown_keys_ignored() {
        // Future keys should not cause errors
        let cmd = parse(b"a=t,i=1,f=32,x=99,y=88,FUTURE=hello;data").unwrap();
        assert_eq!(cmd.action, Action::Transmit);
        assert_eq!(cmd.image_id, Some(1));
    }

    #[test]
    fn parse_empty_payload() {
        let cmd = parse(b"a=q,i=1").unwrap();
        assert!(cmd.payload.is_empty());
    }

    #[test]
    fn parse_empty_control() {
        let cmd = parse(b";data").unwrap();
        assert_eq!(cmd.action, Action::TransmitDisplay);
        assert_eq!(cmd.payload, b"data");
    }

    #[test]
    fn parse_rgb_format() {
        let cmd = parse(b"f=24,s=2,v=2;AAAA").unwrap();
        assert_eq!(cmd.format, Format::Rgb);
    }

    #[test]
    fn parse_image_number() {
        let cmd = parse(b"a=t,I=7,f=100;data").unwrap();
        assert_eq!(cmd.image_number, Some(7));
    }

    #[test]
    fn parse_offsets() {
        let cmd = parse(b"a=p,i=1,X=5,Y=10").unwrap();
        assert_eq!(cmd.x_offset, Some(5));
        assert_eq!(cmd.y_offset, Some(10));
    }

    #[test]
    fn parse_all_actions() {
        assert_eq!(parse(b"a=T").unwrap().action, Action::TransmitDisplay);
        assert_eq!(parse(b"a=t").unwrap().action, Action::Transmit);
        assert_eq!(parse(b"a=p").unwrap().action, Action::Put);
        assert_eq!(parse(b"a=d").unwrap().action, Action::Delete);
        assert_eq!(parse(b"a=q").unwrap().action, Action::Query);
        assert_eq!(parse(b"a=f").unwrap().action, Action::Frame);
        assert_eq!(parse(b"a=a").unwrap().action, Action::Animate);
        assert_eq!(parse(b"a=c").unwrap().action, Action::Compose);
    }

    #[test]
    fn parse_all_transmissions() {
        assert_eq!(parse(b"t=d").unwrap().transmission, Transmission::Direct);
        assert_eq!(parse(b"t=f").unwrap().transmission, Transmission::File);
        assert_eq!(parse(b"t=t").unwrap().transmission, Transmission::TempFile);
        assert_eq!(
            parse(b"t=s").unwrap().transmission,
            Transmission::SharedMemory
        );
    }

    #[test]
    fn parse_quiet_modes() {
        assert_eq!(parse(b"q=0").unwrap().quiet, 0);
        assert_eq!(parse(b"q=1").unwrap().quiet, 1);
        assert_eq!(parse(b"q=2").unwrap().quiet, 2);
    }

    #[test]
    fn error_unknown_action() {
        assert!(parse(b"a=x").is_err());
    }

    #[test]
    fn error_unknown_format() {
        assert!(parse(b"f=99").is_err());
    }

    #[test]
    fn error_invalid_int() {
        assert!(parse(b"i=abc").is_err());
    }

    #[test]
    fn error_invalid_control_pair() {
        assert!(parse(b"noequals").is_err());
    }
}
