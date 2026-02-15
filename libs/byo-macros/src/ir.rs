//! Intermediate representation for the `byo!`/`byo_write!` macro.
//!
//! The parser produces [`IrCommand`] trees; the codegen transforms them into
//! Rust `TokenStream`s that call the `byo` emitter API. This IR decouples
//! parsing from code generation, making both easier to test independently.

use proc_macro2::TokenStream;

/// A value that is either a compile-time literal or a runtime expression.
pub enum IrValue {
    Literal(String),
    Interpolation(TokenStream),
}

/// A property in the IR.
pub enum IrProp {
    /// `key=value`
    Value { key: String, value: IrValue },
    /// `key` (bare boolean flag)
    Boolean { key: String },
    /// `~key` (remove)
    Remove { key: String },
    /// `if cond { props... } [else { props... }]`
    Conditional {
        condition: TokenStream,
        then_props: Vec<IrProp>,
        else_props: Option<Vec<IrProp>>,
    },
}

/// A command in the IR.
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
    /// `!sub seq type`
    Sub { seq: IrValue, target_type: IrValue },
    /// `!unsub seq type`
    Unsub { seq: IrValue, target_type: IrValue },
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
}
