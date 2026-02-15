//! Proc macros for the BYO/OS protocol.
//!
//! Provides [`byo!`] and [`byo_write!`] macros that accept near-wire-syntax
//! BYO/OS commands with Rust expression interpolation, conditionals, and loops.
//! Re-exported from the `byo` crate (behind the `macros` feature, on by default).

mod codegen;
mod ir;
mod parse;

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

fn syn_err(msg: String) -> TokenStream {
    let msg = msg.as_str();
    let ts: proc_macro2::TokenStream = quote::quote! {
        ::std::compile_error!(#msg)
    };
    ts.into()
}
