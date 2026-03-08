//! Code generator: IR → `Vec<Command>`.
//!
//! Parallel to [`codegen`](super::codegen) but generates `Command` enum
//! constructor expressions instead of `Emitter` method calls. Produces
//! values rather than I/O side effects.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::ir::{IrCommand, IrProp, IrValue};

/// State for generating unique variable names within a single macro invocation.
struct Codegen {
    counter: usize,
}

impl Codegen {
    fn new() -> Self {
        Self { counter: 0 }
    }

    fn fresh_ident(&mut self, prefix: &str) -> proc_macro2::Ident {
        let id = format_ident!("__{prefix}_{}", self.counter);
        self.counter += 1;
        id
    }

    /// Returns whether commands require a Vec accumulator (control flow or children present).
    fn needs_accumulator(cmds: &[IrCommand]) -> bool {
        cmds.iter().any(|cmd| {
            matches!(
                cmd,
                IrCommand::Conditional { .. }
                    | IrCommand::ForLoop { .. }
                    | IrCommand::Match { .. }
                    | IrCommand::Upsert {
                        children: Some(_),
                        ..
                    }
                    | IrCommand::Patch {
                        children: Some(_),
                        ..
                    }
                    | IrCommand::Response {
                        children: Some(_),
                        ..
                    }
                    | IrCommand::Message {
                        children: Some(_),
                        ..
                    }
                    | IrCommand::SlotBlock { .. }
            )
        })
    }

    /// Generate a comma-separated list of Command expressions (for simple cases)
    /// or a block that builds and returns a Vec<Command> (for complex cases).
    fn gen_commands_expr(&mut self, cmds: &[IrCommand]) -> TokenStream {
        if Self::needs_accumulator(cmds) {
            // Complex case: accumulate into a Vec
            let acc = self.fresh_ident("cmds");
            let stmts = self.gen_commands_into(cmds, &acc);
            quote! {
                {
                    let mut #acc = ::std::vec::Vec::<::byo::protocol::Command>::new();
                    #stmts
                    #acc
                }
            }
        } else {
            // Simple case: bare comma-separated expressions
            let exprs: Vec<TokenStream> = cmds
                .iter()
                .map(|cmd| self.gen_simple_command(cmd))
                .collect();
            quote! { #(#exprs),* }
        }
    }

    /// Generate a Vec<Command> expression.
    fn gen_vec_expr(&mut self, cmds: &[IrCommand]) -> TokenStream {
        if Self::needs_accumulator(cmds) {
            let acc = self.fresh_ident("cmds");
            let stmts = self.gen_commands_into(cmds, &acc);
            quote! {
                {
                    let mut #acc = ::std::vec::Vec::<::byo::protocol::Command>::new();
                    #stmts
                    #acc
                }
            }
        } else {
            let exprs: Vec<TokenStream> = cmds
                .iter()
                .map(|cmd| self.gen_simple_command(cmd))
                .collect();
            quote! { ::std::vec![#(#exprs),*] }
        }
    }

    /// Generate statements that push Command values into an accumulator variable.
    fn gen_commands_into(&mut self, cmds: &[IrCommand], acc: &proc_macro2::Ident) -> TokenStream {
        let stmts: Vec<TokenStream> = cmds
            .iter()
            .map(|cmd| self.gen_command_into(cmd, acc))
            .collect();
        quote! { #(#stmts)* }
    }

    /// Generate a push statement for a single command into the accumulator.
    fn gen_command_into(&mut self, cmd: &IrCommand, acc: &proc_macro2::Ident) -> TokenStream {
        match cmd {
            IrCommand::Upsert {
                kind,
                id,
                props,
                children: None,
            } => {
                let expr = self.gen_upsert_or_patch_expr(true, kind, id, props);
                quote! { #acc.push(#expr); }
            }
            IrCommand::Upsert {
                kind,
                id,
                props,
                children: Some(ch),
            } => {
                let expr = self.gen_upsert_or_patch_expr(true, kind, id, props);
                let child_stmts = self.gen_commands_into(ch, acc);
                quote! {
                    #acc.push(#expr);
                    #acc.push(::byo::protocol::Command::Push { slot: ::std::option::Option::None });
                    #child_stmts
                    #acc.push(::byo::protocol::Command::Pop);
                }
            }
            IrCommand::Patch {
                kind,
                id,
                props,
                children: None,
            } => {
                let expr = self.gen_upsert_or_patch_expr(false, kind, id, props);
                quote! { #acc.push(#expr); }
            }
            IrCommand::Patch {
                kind,
                id,
                props,
                children: Some(ch),
            } => {
                let expr = self.gen_upsert_or_patch_expr(false, kind, id, props);
                let child_stmts = self.gen_commands_into(ch, acc);
                quote! {
                    #acc.push(#expr);
                    #acc.push(::byo::protocol::Command::Push { slot: ::std::option::Option::None });
                    #child_stmts
                    #acc.push(::byo::protocol::Command::Pop);
                }
            }
            IrCommand::Destroy { kind, id } => {
                let kind_expr = self.gen_bytestr_value(kind);
                let id_expr = self.gen_bytestr_value(id);
                quote! {
                    #acc.push(::byo::protocol::Command::Destroy {
                        kind: #kind_expr,
                        id: #id_expr,
                    });
                }
            }
            IrCommand::Event {
                kind,
                seq,
                id,
                props,
            } => {
                let kind_expr = self.gen_event_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let id_expr = self.gen_bytestr_value(id);
                let props_expr = self.gen_props_expr(props);
                quote! {
                    #acc.push(::byo::protocol::Command::Event {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        id: #id_expr,
                        props: #props_expr,
                    });
                }
            }
            IrCommand::Ack { kind, seq, props } => {
                let kind_expr = self.gen_event_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let props_expr = self.gen_props_expr(props);
                quote! {
                    #acc.push(::byo::protocol::Command::Ack {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        props: #props_expr,
                    });
                }
            }
            IrCommand::Pragma { kind, targets } => {
                let pragma_expr = self.gen_pragma_expr(kind, targets);
                quote! {
                    #acc.push(::byo::protocol::Command::Pragma(#pragma_expr));
                }
            }
            IrCommand::Request {
                kind,
                seq,
                targets,
                props,
            } => {
                let kind_expr = self.gen_request_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let target_exprs: Vec<TokenStream> =
                    targets.iter().map(|t| self.gen_bytestr_value(t)).collect();
                let props_expr = self.gen_props_expr(props);
                quote! {
                    #acc.push(::byo::protocol::Command::Request {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        targets: ::std::vec![#(#target_exprs),*],
                        props: #props_expr,
                    });
                }
            }
            IrCommand::Response {
                kind,
                seq,
                props,
                children,
            } => {
                let kind_expr = self.gen_response_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let props_expr = self.gen_props_expr(props);
                let body_expr = match children {
                    Some(ch) => {
                        let vec_expr = self.gen_vec_expr(ch);
                        quote! { ::std::option::Option::Some(#vec_expr) }
                    }
                    None => quote! { ::std::option::Option::None },
                };
                quote! {
                    #acc.push(::byo::protocol::Command::Response {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        props: #props_expr,
                        body: #body_expr,
                    });
                }
            }
            IrCommand::Message {
                kind,
                target,
                props,
                children,
            } => {
                let kind_expr = self.gen_bytestr_value(kind);
                let target_expr = self.gen_bytestr_value(target);
                let props_expr = self.gen_props_expr(props);
                let body_expr = match children {
                    Some(ch) => {
                        let vec_expr = self.gen_vec_expr(ch);
                        quote! { ::std::option::Option::Some(#vec_expr) }
                    }
                    None => quote! { ::std::option::Option::None },
                };
                quote! {
                    #acc.push(::byo::protocol::Command::Message {
                        kind: #kind_expr,
                        target: #target_expr,
                        props: #props_expr,
                        body: #body_expr,
                    });
                }
            }
            IrCommand::SlotBlock { name, children } => {
                let name_expr = self.gen_bytestr_value(name);
                let child_stmts = self.gen_commands_into(children, acc);
                quote! {
                    #acc.push(::byo::protocol::Command::Push {
                        slot: ::std::option::Option::Some(#name_expr),
                    });
                    #child_stmts
                    #acc.push(::byo::protocol::Command::Pop);
                }
            }
            IrCommand::Conditional {
                condition,
                then_cmds,
                else_cmds,
            } => {
                let then_code = self.gen_commands_into(then_cmds, acc);
                if let Some(else_cmds) = else_cmds {
                    let else_code = self.gen_commands_into(else_cmds, acc);
                    quote! {
                        if #condition {
                            #then_code
                        } else {
                            #else_code
                        }
                    }
                } else {
                    quote! {
                        if #condition {
                            #then_code
                        }
                    }
                }
            }
            IrCommand::ForLoop { pat, iter, body } => {
                let body_code = self.gen_commands_into(body, acc);
                quote! {
                    for #pat in #iter {
                        #body_code
                    }
                }
            }
            IrCommand::Match { expr, arms } => {
                let arm_code: Vec<TokenStream> = arms
                    .iter()
                    .map(|arm| {
                        let pat = &arm.pattern;
                        let body = self.gen_commands_into(&arm.body, acc);
                        quote! { #pat => { #body } }
                    })
                    .collect();
                quote! {
                    match #expr {
                        #(#arm_code)*
                    }
                }
            }
        }
    }

    /// Generate a simple Command expression (no accumulator needed).
    /// Only called for commands without children or control flow.
    fn gen_simple_command(&mut self, cmd: &IrCommand) -> TokenStream {
        match cmd {
            IrCommand::Upsert {
                kind,
                id,
                props,
                children: None,
            } => self.gen_upsert_or_patch_expr(true, kind, id, props),
            IrCommand::Patch {
                kind,
                id,
                props,
                children: None,
            } => self.gen_upsert_or_patch_expr(false, kind, id, props),
            IrCommand::Destroy { kind, id } => {
                let kind_expr = self.gen_bytestr_value(kind);
                let id_expr = self.gen_bytestr_value(id);
                quote! {
                    ::byo::protocol::Command::Destroy {
                        kind: #kind_expr,
                        id: #id_expr,
                    }
                }
            }
            IrCommand::Event {
                kind,
                seq,
                id,
                props,
            } => {
                let kind_expr = self.gen_event_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let id_expr = self.gen_bytestr_value(id);
                let props_expr = self.gen_props_expr(props);
                quote! {
                    ::byo::protocol::Command::Event {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        id: #id_expr,
                        props: #props_expr,
                    }
                }
            }
            IrCommand::Ack { kind, seq, props } => {
                let kind_expr = self.gen_event_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let props_expr = self.gen_props_expr(props);
                quote! {
                    ::byo::protocol::Command::Ack {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        props: #props_expr,
                    }
                }
            }
            IrCommand::Pragma { kind, targets } => {
                let pragma_expr = self.gen_pragma_expr(kind, targets);
                quote! {
                    ::byo::protocol::Command::Pragma(#pragma_expr)
                }
            }
            IrCommand::Request {
                kind,
                seq,
                targets,
                props,
            } => {
                let kind_expr = self.gen_request_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let target_exprs: Vec<TokenStream> =
                    targets.iter().map(|t| self.gen_bytestr_value(t)).collect();
                let props_expr = self.gen_props_expr(props);
                quote! {
                    ::byo::protocol::Command::Request {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        targets: ::std::vec![#(#target_exprs),*],
                        props: #props_expr,
                    }
                }
            }
            IrCommand::Response {
                kind,
                seq,
                props,
                children,
            } => {
                let kind_expr = self.gen_response_kind(kind);
                let seq_expr = self.gen_u64_value(seq);
                let props_expr = self.gen_props_expr(props);
                let body_expr = match children {
                    Some(ch) => {
                        let vec_expr = self.gen_vec_expr(ch);
                        quote! { ::std::option::Option::Some(#vec_expr) }
                    }
                    None => quote! { ::std::option::Option::None },
                };
                quote! {
                    ::byo::protocol::Command::Response {
                        kind: #kind_expr,
                        seq: #seq_expr,
                        props: #props_expr,
                        body: #body_expr,
                    }
                }
            }
            IrCommand::Message {
                kind,
                target,
                props,
                children,
            } => {
                let kind_expr = self.gen_bytestr_value(kind);
                let target_expr = self.gen_bytestr_value(target);
                let props_expr = self.gen_props_expr(props);
                let body_expr = match children {
                    Some(ch) => {
                        let vec_expr = self.gen_vec_expr(ch);
                        quote! { ::std::option::Option::Some(#vec_expr) }
                    }
                    None => quote! { ::std::option::Option::None },
                };
                quote! {
                    ::byo::protocol::Command::Message {
                        kind: #kind_expr,
                        target: #target_expr,
                        props: #props_expr,
                        body: #body_expr,
                    }
                }
            }
            // These should not appear in simple mode, but handle gracefully
            _ => unreachable!("gen_simple_command called for command that needs accumulator"),
        }
    }

    /// Generate an Upsert or Patch Command expression.
    fn gen_upsert_or_patch_expr(
        &mut self,
        is_upsert: bool,
        kind: &IrValue,
        id: &IrValue,
        props: &[IrProp],
    ) -> TokenStream {
        let kind_expr = self.gen_bytestr_value(kind);
        let id_expr = self.gen_bytestr_value(id);
        let props_expr = self.gen_props_expr(props);
        if is_upsert {
            quote! {
                ::byo::protocol::Command::Upsert {
                    kind: #kind_expr,
                    id: #id_expr,
                    props: #props_expr,
                }
            }
        } else {
            quote! {
                ::byo::protocol::Command::Patch {
                    kind: #kind_expr,
                    id: #id_expr,
                    props: #props_expr,
                }
            }
        }
    }

    /// Generate a ByteStr value expression from an IrValue.
    fn gen_bytestr_value(&mut self, val: &IrValue) -> TokenStream {
        match val {
            IrValue::Literal(s) => {
                quote! { ::byo::byte_str::ByteStr::from(#s) }
            }
            IrValue::Interpolation(expr) => {
                let v = self.fresh_ident("v");
                quote! {
                    {
                        let #v = #expr;
                        ::byo::byte_str::ByteStr::from(#v.to_string())
                    }
                }
            }
        }
    }

    /// Generate a u64 value expression from an IrValue (for sequence numbers).
    fn gen_u64_value(&self, val: &IrValue) -> TokenStream {
        match val {
            IrValue::Literal(s) => {
                let n: u64 = s.parse().expect("sequence number must be a valid u64");
                quote! { #n }
            }
            IrValue::Interpolation(expr) => quote! { #expr },
        }
    }

    /// Generate an EventKind expression from an IrValue.
    fn gen_event_kind(&mut self, val: &IrValue) -> TokenStream {
        match val {
            IrValue::Literal(s) => {
                quote! { ::byo::protocol::EventKind::from_wire(#s) }
            }
            IrValue::Interpolation(expr) => {
                let v = self.fresh_ident("v");
                quote! {
                    {
                        let #v = #expr;
                        ::byo::protocol::EventKind::from_wire(#v.to_string())
                    }
                }
            }
        }
    }

    /// Generate a `PragmaKind` expression from IR-level kind and targets.
    fn gen_pragma_expr(&mut self, kind: &IrValue, targets: &[IrValue]) -> TokenStream {
        if let IrValue::Literal(k) = kind {
            let target_exprs: Vec<TokenStream> =
                targets.iter().map(|t| self.gen_bytestr_value(t)).collect();
            match k.as_str() {
                "claim" => {
                    quote! { ::byo::protocol::PragmaKind::Claim(::std::vec![#(#target_exprs),*]) }
                }
                "unclaim" => {
                    quote! { ::byo::protocol::PragmaKind::Unclaim(::std::vec![#(#target_exprs),*]) }
                }
                "observe" => {
                    quote! { ::byo::protocol::PragmaKind::Observe(::std::vec![#(#target_exprs),*]) }
                }
                "unobserve" => {
                    quote! { ::byo::protocol::PragmaKind::Unobserve(::std::vec![#(#target_exprs),*]) }
                }
                "redirect" => {
                    let target = &target_exprs[0];
                    quote! { ::byo::protocol::PragmaKind::Redirect(#target) }
                }
                "unredirect" => quote! { ::byo::protocol::PragmaKind::Unredirect },
                "handle" | "unhandle" => {
                    // Handle targets are combined "type?request" strings in the IR.
                    // Split them at compile time into (ByteStr, ByteStr) tuples.
                    let pair_exprs: Vec<TokenStream> = targets.iter().map(|t| {
                        match t {
                            IrValue::Literal(s) => {
                                let (ty, req) = s.split_once('?').expect("handle target must contain '?'");
                                quote! { (::byo::byte_str::ByteStr::from(#ty), ::byo::byte_str::ByteStr::from(#req)) }
                            }
                            IrValue::Interpolation(expr) => {
                                let v = self.fresh_ident("v");
                                quote! {
                                    {
                                        let #v: &str = (#expr).as_ref();
                                        let (t, r) = #v.split_once('?').expect("handle target must contain '?'");
                                        (::byo::byte_str::ByteStr::from(t), ::byo::byte_str::ByteStr::from(r))
                                    }
                                }
                            }
                        }
                    }).collect();
                    if k == "handle" {
                        quote! { ::byo::protocol::PragmaKind::Handle(::std::vec![#(#pair_exprs),*]) }
                    } else {
                        quote! { ::byo::protocol::PragmaKind::Unhandle(::std::vec![#(#pair_exprs),*]) }
                    }
                }
                _ => {
                    let name_expr = self.gen_bytestr_value(kind);
                    quote! {
                        ::byo::protocol::PragmaKind::Other {
                            name: #name_expr,
                            targets: ::std::vec![#(#target_exprs),*],
                        }
                    }
                }
            }
        } else {
            // Dynamic kind — must use Other variant
            let name_expr = self.gen_bytestr_value(kind);
            let target_exprs: Vec<TokenStream> =
                targets.iter().map(|t| self.gen_bytestr_value(t)).collect();
            quote! {
                ::byo::protocol::PragmaKind::Other {
                    name: #name_expr,
                    targets: ::std::vec![#(#target_exprs),*],
                }
            }
        }
    }

    /// Generate a RequestKind expression from an IrValue.
    fn gen_request_kind(&mut self, val: &IrValue) -> TokenStream {
        match val {
            IrValue::Literal(s) => {
                quote! { ::byo::protocol::RequestKind::from_wire(#s) }
            }
            IrValue::Interpolation(expr) => {
                let v = self.fresh_ident("v");
                quote! {
                    {
                        let #v = #expr;
                        ::byo::protocol::RequestKind::from_wire(#v.to_string())
                    }
                }
            }
        }
    }

    /// Generate a ResponseKind expression from an IrValue.
    fn gen_response_kind(&mut self, val: &IrValue) -> TokenStream {
        match val {
            IrValue::Literal(s) => {
                quote! { ::byo::protocol::ResponseKind::from_wire(#s) }
            }
            IrValue::Interpolation(expr) => {
                let v = self.fresh_ident("v");
                quote! {
                    {
                        let #v = #expr;
                        ::byo::protocol::ResponseKind::from_wire(#v.to_string())
                    }
                }
            }
        }
    }

    /// Generate a props expression — either a static vec![...] or a dynamic accumulator block.
    fn gen_props_expr(&mut self, props: &[IrProp]) -> TokenStream {
        if props.is_empty() {
            return quote! { ::std::vec::Vec::new() };
        }

        if has_dynamic_props(props) {
            let acc = self.fresh_ident("props");
            let push_stmts = self.gen_dynamic_prop_pushes(props, &acc);
            quote! {
                {
                    let mut #acc = ::std::vec::Vec::<::byo::protocol::Prop>::new();
                    #push_stmts
                    #acc
                }
            }
        } else {
            let items = self.gen_static_prop_items(props);
            quote! { ::std::vec![#(#items),*] }
        }
    }

    /// Generate static prop items for a vec literal.
    fn gen_static_prop_items(&self, props: &[IrProp]) -> Vec<TokenStream> {
        props
            .iter()
            .map(|p| match p {
                IrProp::Value {
                    key,
                    value: IrValue::Literal(v),
                } => {
                    quote! { ::byo::protocol::Prop::val(#key, #v) }
                }
                IrProp::Boolean { key } => {
                    quote! { ::byo::protocol::Prop::flag(#key) }
                }
                IrProp::Remove { key } => {
                    quote! { ::byo::protocol::Prop::remove(#key) }
                }
                _ => unreachable!("has_dynamic_props was false"),
            })
            .collect()
    }

    /// Generate push statements for dynamic props into a Vec variable.
    fn gen_dynamic_prop_pushes(
        &mut self,
        props: &[IrProp],
        vec_ident: &proc_macro2::Ident,
    ) -> TokenStream {
        let stmts: Vec<TokenStream> = props
            .iter()
            .map(|p| self.gen_prop_push(p, vec_ident))
            .collect();
        quote! { #(#stmts)* }
    }

    /// Generate a push statement for a single prop.
    fn gen_prop_push(&mut self, prop: &IrProp, vec_ident: &proc_macro2::Ident) -> TokenStream {
        match prop {
            IrProp::Value {
                key,
                value: IrValue::Literal(v),
            } => {
                quote! { #vec_ident.push(::byo::protocol::Prop::val(#key, #v)); }
            }
            IrProp::Value {
                key,
                value: IrValue::Interpolation(expr),
            } => {
                quote! { #vec_ident.push(::byo::protocol::Prop::val(#key, #expr)); }
            }
            IrProp::Boolean { key } => {
                quote! { #vec_ident.push(::byo::protocol::Prop::flag(#key)); }
            }
            IrProp::Remove { key } => {
                quote! { #vec_ident.push(::byo::protocol::Prop::remove(#key)); }
            }
            IrProp::Spread { expr } => {
                quote! { #vec_ident.extend(::byo::props::ToProps::to_props(&#expr)); }
            }
            IrProp::Conditional {
                condition,
                then_props,
                else_props,
            } => {
                let then_pushes = self.gen_dynamic_prop_pushes(then_props, vec_ident);
                if let Some(else_props) = else_props {
                    let else_pushes = self.gen_dynamic_prop_pushes(else_props, vec_ident);
                    quote! {
                        if #condition {
                            #then_pushes
                        } else {
                            #else_pushes
                        }
                    }
                } else {
                    quote! {
                        if #condition {
                            #then_pushes
                        }
                    }
                }
            }
        }
    }
}

/// Check if any prop is dynamic (interpolation, conditional, or spread).
fn has_dynamic_props(props: &[IrProp]) -> bool {
    props.iter().any(|p| {
        matches!(
            p,
            IrProp::Value {
                value: IrValue::Interpolation(_),
                ..
            } | IrProp::Conditional { .. }
                | IrProp::Spread { .. }
        )
    })
}

/// Generate comma-separated `Command` expressions.
///
/// For simple cases (no control flow, no children), produces bare expressions
/// suitable for splicing into `vec![...]` or other contexts. For complex cases,
/// produces a block that returns a `Vec<Command>`.
pub fn gen_byo_commands(cmds: &[IrCommand]) -> TokenStream {
    let mut cg = Codegen::new();
    cg.gen_commands_expr(cmds)
}

/// Generate a `Vec<Command>` expression.
pub fn gen_byo_vec(cmds: &[IrCommand]) -> TokenStream {
    let mut cg = Codegen::new();
    cg.gen_vec_expr(cmds)
}
