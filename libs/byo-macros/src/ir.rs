//! Intermediate representation for the `byo!`/`byo_write!` macro.
//!
//! The parser produces [`IrCommand`] trees; the codegen transforms them into
//! Rust `TokenStream`s that call the `byo` emitter API. This IR decouples
//! parsing from code generation, making both easier to test independently.

use proc_macro2::TokenStream;

/// A value that is either a compile-time literal or a runtime expression.
#[derive(Debug)]
pub enum IrValue {
    Literal(String),
    Interpolation(TokenStream),
}

/// A property in the IR.
#[derive(Debug)]
pub enum IrProp {
    /// `key=value`
    Value { key: String, value: IrValue },
    /// `key` (bare boolean flag)
    Boolean { key: String },
    /// `~key` (remove)
    Remove { key: String },
    /// `{..expr}` — spread props from an IntoProps impl
    Spread { expr: TokenStream },
    /// `if cond { props... } [else { props... }]`
    Conditional {
        condition: TokenStream,
        then_props: Vec<IrProp>,
        else_props: Option<Vec<IrProp>>,
    },
}

/// A command in the IR.
#[derive(Debug)]
pub enum IrCommand {
    /// `+type id props... [{ children }]`
    Upsert {
        kind: IrValue,
        id: IrValue,
        props: Vec<IrProp>,
        children: Option<Vec<IrCommand>>,
    },
    /// `-type id`
    Destroy { kind: IrValue, id: IrValue },
    /// `@type id props... [{ children }]`
    Patch {
        kind: IrValue,
        id: IrValue,
        props: Vec<IrProp>,
        children: Option<Vec<IrCommand>>,
    },
    /// `!kind seq id props...`
    Event {
        kind: IrValue,
        seq: IrValue,
        id: IrValue,
        props: Vec<IrProp>,
    },
    /// `!ack kind seq props...`
    Ack {
        kind: IrValue,
        seq: IrValue,
        props: Vec<IrProp>,
    },
    /// `#kind targets...` — Stream pragma (fire-and-forget, no seq).
    Pragma {
        kind: IrValue,
        targets: Vec<IrValue>,
    },
    /// `?kind seq target props...`
    Request {
        kind: IrValue,
        seq: IrValue,
        targets: Vec<IrValue>,
        props: Vec<IrProp>,
    },
    /// `.kind seq props... [{ children }]`
    Response {
        kind: IrValue,
        seq: IrValue,
        props: Vec<IrProp>,
        children: Option<Vec<IrCommand>>,
    },
    /// `.kind target props... [{ children }]` — standalone message (no seq)
    Message {
        kind: IrValue,
        target: IrValue,
        props: Vec<IrProp>,
        children: Option<Vec<IrCommand>>,
    },
    /// `if cond { cmds... } [else { cmds... }]`
    Conditional {
        condition: TokenStream,
        then_cmds: Vec<IrCommand>,
        else_cmds: Option<Vec<IrCommand>>,
    },
    /// `for pat in iter { cmds... }`
    ForLoop {
        pat: TokenStream,
        iter: TokenStream,
        body: Vec<IrCommand>,
    },
    /// `match expr { pat => { cmds... }, ... }`
    Match {
        expr: TokenStream,
        arms: Vec<MatchArm>,
    },
    /// `{name ... }` — Slot block (within children of a +/@ command)
    SlotBlock {
        name: IrValue,
        children: Vec<IrCommand>,
    },
}

/// A single arm in a `match` expression.
#[derive(Debug)]
pub struct MatchArm {
    pub pattern: TokenStream,
    pub body: Vec<IrCommand>,
}
