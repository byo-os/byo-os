//! Proc macros for the BYO/OS protocol.
//!
//! Provides [`byo!`] and [`byo_write!`] macros that accept near-wire-syntax
//! BYO/OS commands with Rust expression interpolation, conditionals, and loops.
//! Re-exported from the `byo` crate (behind the `macros` feature, on by default).

mod codegen;
mod codegen_commands;
mod derive_props;
mod ir;
mod parse;
mod serialize;

use proc_macro::TokenStream;

/// Emit a framed APC batch to stdout. Panics on write error.
#[proc_macro]
pub fn byo(input: TokenStream) -> TokenStream {
    let input2: proc_macro2::TokenStream = input.into();
    let cmds = match parse::parse(input2, true) {
        Ok(cmds) => cmds,
        Err(e) => return syn_err(e),
    };
    codegen::gen_byo(&cmds).into()
}

/// Emit commands to an existing `Emitter` (no framing). Returns `io::Result<()>`.
#[proc_macro]
pub fn byo_write(input: TokenStream) -> TokenStream {
    let input2: proc_macro2::TokenStream = input.into();
    let mut iter = input2.into_iter();

    // First token(s): the emitter expression (an ident)
    let em_expr: proc_macro2::TokenStream = match iter.next() {
        Some(tt) => {
            let ts: proc_macro2::TokenStream = std::iter::once(tt).collect();
            ts
        }
        None => return syn_err("expected emitter expression".to_string()),
    };

    // Comma separator
    match iter.next() {
        Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == ',' => {}
        _ => return syn_err("expected `,` after emitter expression".to_string()),
    }

    // Remaining tokens: the commands
    let rest: proc_macro2::TokenStream = iter.collect();
    let cmds = match parse::parse(rest, true) {
        Ok(cmds) => cmds,
        Err(e) => return syn_err(e),
    };
    codegen::gen_byo_write(em_expr, &cmds).into()
}

/// Serialize BYO DSL to a `&'static str` at compile time.
///
/// Same syntax as `byo!` but produces a string literal instead of emitter calls.
/// Does not support interpolation, conditionals, or loops — all values must be
/// literals.
///
/// ```ignore
/// let expected: &str = byo_str! { +view sidebar class="w-64" };
/// ```
#[proc_macro]
pub fn byo_str(input: TokenStream) -> TokenStream {
    let input2: proc_macro2::TokenStream = input.into();
    let cmds = match parse::parse(input2, true) {
        Ok(cmds) => cmds,
        Err(e) => return syn_err(e),
    };
    let canonical = match serialize::serialize_commands(&cmds) {
        Ok(s) => s,
        Err(e) => return syn_err(e),
    };
    let ts: proc_macro2::TokenStream = quote::quote! { #canonical };
    ts.into()
}

/// Assert that actual BYO output matches expected DSL structurally.
///
/// First argument is the variable holding actual output (`&str` or `&[u8]`).
/// Remaining tokens are the expected BYO DSL (literals only).
///
/// ```ignore
/// byo_assert_eq!(output, {
///     +view sidebar class="w-64" {
///         +text label content="Hello"
///     }
/// });
/// ```
#[proc_macro]
pub fn byo_assert_eq(input: TokenStream) -> TokenStream {
    let input2: proc_macro2::TokenStream = input.into();
    let mut iter = input2.into_iter();

    // First token: the actual string variable
    let actual_expr: proc_macro2::TokenStream = match iter.next() {
        Some(tt) => std::iter::once(tt).collect(),
        None => return syn_err("expected actual output expression".to_string()),
    };

    // Comma separator
    match iter.next() {
        Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == ',' => {}
        _ => return syn_err("expected `,` after actual expression".to_string()),
    }

    // Remaining tokens: the expected BYO DSL
    let rest: proc_macro2::TokenStream = iter.collect();
    let cmds = match parse::parse(rest, true) {
        Ok(cmds) => cmds,
        Err(e) => return syn_err(e),
    };
    let canonical = match serialize::serialize_commands(&cmds) {
        Ok(s) => s,
        Err(e) => return syn_err(e),
    };

    let ts: proc_macro2::TokenStream = quote::quote! {
        ::byo::assert::assert_eq(&#actual_expr, #canonical)
    };
    ts.into()
}

/// Build comma-separated `Command` enum expressions from BYO DSL.
///
/// For simple cases (no control flow, no children), produces bare expressions
/// suitable for splicing into `vec![...]`. For complex cases, produces a block
/// returning a `Vec<Command>`.
#[proc_macro]
pub fn byo_commands(input: TokenStream) -> TokenStream {
    let input2: proc_macro2::TokenStream = input.into();
    let cmds = match parse::parse(input2, true) {
        Ok(cmds) => cmds,
        Err(e) => return syn_err(e),
    };
    codegen_commands::gen_byo_commands(&cmds).into()
}

/// Build a `Vec<Command>` from BYO DSL.
#[proc_macro]
pub fn byo_vec(input: TokenStream) -> TokenStream {
    let input2: proc_macro2::TokenStream = input.into();
    let cmds = match parse::parse(input2, true) {
        Ok(cmds) => cmds,
        Err(e) => return syn_err(e),
    };
    codegen_commands::gen_byo_vec(&cmds).into()
}

/// Derive `FromProps` for a struct — builds the struct from a `&[Prop]` slice.
#[proc_macro_derive(FromProps, attributes(prop))]
pub fn derive_from_props(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    derive_props::derive_from_props(input).into()
}

/// Derive `ToProps` for a struct — converts the struct into a `Vec<Prop>`.
#[proc_macro_derive(ToProps, attributes(prop))]
pub fn derive_to_props(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    derive_props::derive_to_props(input).into()
}

/// Derive `ReadProp` for a unit enum — parses a prop value into a variant.
#[proc_macro_derive(ReadProp, attributes(prop))]
pub fn derive_read_prop(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    derive_props::derive_read_prop(input).into()
}

/// Derive `WriteProp` for a unit enum — encodes a variant as a prop value.
#[proc_macro_derive(WriteProp, attributes(prop))]
pub fn derive_write_prop(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    derive_props::derive_write_prop(input).into()
}

fn syn_err(msg: String) -> TokenStream {
    let msg = msg.as_str();
    let ts: proc_macro2::TokenStream = quote::quote! {
        ::std::compile_error!(#msg)
    };
    ts.into()
}
