//! Code generator: IR → `TokenStream`.
//!
//! Transforms [`IrCommand`] trees into Rust code that calls the `byo`
//! emitter API. Handles two optimization levels for props:
//!
//! - **Static**: all-literal props emit a `&[Prop::val(...), ...]` slice.
//! - **Dynamic**: interpolated or conditional props use a `Vec` with pushes.
//!
//! Children blocks generate `upsert_with`/`patch_with` closures for
//! compile-time balanced `{}`/`}`.

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

    /// Generate code for a list of commands. `em_ident` is the emitter variable name.
    fn gen_commands(&mut self, cmds: &[IrCommand], em_ident: &proc_macro2::Ident) -> TokenStream {
        let stmts: Vec<TokenStream> = cmds
            .iter()
            .map(|cmd| self.gen_command(cmd, em_ident))
            .collect();
        quote! { #(#stmts)* }
    }

    /// Generate code for a single command.
    fn gen_command(&mut self, cmd: &IrCommand, em_ident: &proc_macro2::Ident) -> TokenStream {
        match cmd {
            IrCommand::Upsert {
                kind,
                id,
                props,
                children,
            } => self.gen_upsert_or_patch(true, kind, id, props, children, em_ident),
            IrCommand::Patch {
                kind,
                id,
                props,
                children,
            } => self.gen_upsert_or_patch(false, kind, id, props, children, em_ident),
            IrCommand::Destroy { kind, id } => {
                let (kind_binds, kind_expr) = self.gen_str_value(kind);
                let (id_binds, id_expr) = self.gen_str_value(id);
                quote! {
                    #kind_binds
                    #id_binds
                    #em_ident.destroy(#kind_expr, #id_expr)?;
                }
            }
            IrCommand::Event {
                kind,
                seq,
                id,
                props,
            } => {
                let (kind_binds, kind_expr) = self.gen_str_value(kind);
                let (seq_binds, seq_expr) = self.gen_seq_value(seq);
                let (id_binds, id_expr) = self.gen_str_value(id);
                if has_dynamic_props(props) {
                    let props_var = self.fresh_ident("props");
                    let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                    quote! {
                        {
                            #kind_binds
                            #seq_binds
                            #id_binds
                            let mut #props_var = ::std::vec::Vec::new();
                            #push_stmts
                            #em_ident.event(#kind_expr, #seq_expr, #id_expr, &#props_var)?;
                        }
                    }
                } else {
                    let prop_items = self.gen_static_prop_items(props);
                    quote! {
                        #kind_binds
                        #seq_binds
                        #id_binds
                        #em_ident.event(#kind_expr, #seq_expr, #id_expr, &[#(#prop_items),*])?;
                    }
                }
            }
            IrCommand::Ack { kind, seq, props } => {
                let (kind_binds, kind_expr) = self.gen_str_value(kind);
                let (seq_binds, seq_expr) = self.gen_seq_value(seq);
                if has_dynamic_props(props) {
                    let props_var = self.fresh_ident("props");
                    let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                    quote! {
                        {
                            #kind_binds
                            #seq_binds
                            let mut #props_var = ::std::vec::Vec::new();
                            #push_stmts
                            #em_ident.ack(#kind_expr, #seq_expr, &#props_var)?;
                        }
                    }
                } else {
                    let prop_items = self.gen_static_prop_items(props);
                    quote! {
                        #kind_binds
                        #seq_binds
                        #em_ident.ack(#kind_expr, #seq_expr, &[#(#prop_items),*])?;
                    }
                }
            }
            IrCommand::Pragma { kind, targets } => self.gen_pragma(kind, targets, em_ident),
            IrCommand::Request {
                kind,
                seq,
                targets,
                props,
            } => self.gen_request(kind, seq, targets, props, em_ident),
            IrCommand::Response {
                kind,
                seq,
                props,
                children,
            } => self.gen_response(kind, seq, props, children, em_ident),
            IrCommand::Message {
                kind,
                target,
                props,
                children,
            } => self.gen_message(kind, target, props, children, em_ident),
            IrCommand::Conditional {
                condition,
                then_cmds,
                else_cmds,
            } => {
                let then_code = self.gen_commands(then_cmds, em_ident);
                if let Some(else_cmds) = else_cmds {
                    let else_code = self.gen_commands(else_cmds, em_ident);
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
                let body_code = self.gen_commands(body, em_ident);
                quote! {
                    for #pat in #iter {
                        #body_code
                    }
                }
            }
            IrCommand::SlotBlock { name, children } => {
                let (name_binds, name_expr) = self.gen_str_value(name);
                let child_code = self.gen_commands(children, em_ident);
                quote! {
                    #name_binds
                    #em_ident.slot_push(#name_expr)?;
                    #child_code
                    #em_ident.pop()?;
                }
            }
            IrCommand::Match { expr, arms } => {
                let arm_code: Vec<TokenStream> = arms
                    .iter()
                    .map(|arm| {
                        let pat = &arm.pattern;
                        let body = self.gen_commands(&arm.body, em_ident);
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

    /// Generate upsert or patch. Shared logic since they're structurally identical.
    fn gen_upsert_or_patch(
        &mut self,
        is_upsert: bool,
        kind: &IrValue,
        id: &IrValue,
        props: &[IrProp],
        children: &Option<Vec<IrCommand>>,
        em_ident: &proc_macro2::Ident,
    ) -> TokenStream {
        let (kind_binds, kind_expr) = self.gen_str_value(kind);
        let (id_binds, id_expr) = self.gen_str_value(id);
        let is_dynamic = has_dynamic_props(props);

        match (children, is_dynamic) {
            // No children, static props → simple call with slice literal
            (None, false) => {
                let prop_items = self.gen_static_prop_items(props);
                let method = if is_upsert {
                    quote! { upsert }
                } else {
                    quote! { patch }
                };
                quote! {
                    #kind_binds
                    #id_binds
                    #em_ident.#method(#kind_expr, #id_expr, &[#(#prop_items),*])?;
                }
            }
            // No children, dynamic props → Vec path
            (None, true) => {
                let props_var = self.fresh_ident("props");
                let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                let method = if is_upsert {
                    quote! { upsert }
                } else {
                    quote! { patch }
                };
                quote! {
                    {
                        #kind_binds
                        #id_binds
                        let mut #props_var = ::std::vec::Vec::new();
                        #push_stmts
                        #em_ident.#method(#kind_expr, #id_expr, &#props_var)?;
                    }
                }
            }
            // Children, static props
            (Some(ch), false) => {
                let prop_items = self.gen_static_prop_items(props);
                let child_code = self.gen_commands(ch, em_ident);
                let method = if is_upsert {
                    quote! { upsert_with }
                } else {
                    quote! { patch_with }
                };
                quote! {
                    #kind_binds
                    #id_binds
                    #em_ident.#method(#kind_expr, #id_expr, &[#(#prop_items),*], |#em_ident| {
                        #child_code
                        ::std::result::Result::Ok(())
                    })?;
                }
            }
            // Children, dynamic props
            (Some(ch), true) => {
                let props_var = self.fresh_ident("props");
                let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                let child_code = self.gen_commands(ch, em_ident);
                let method = if is_upsert {
                    quote! { upsert_with }
                } else {
                    quote! { patch_with }
                };
                quote! {
                    {
                        #kind_binds
                        #id_binds
                        let mut #props_var = ::std::vec::Vec::new();
                        #push_stmts
                        #em_ident.#method(#kind_expr, #id_expr, &#props_var, |#em_ident| {
                            #child_code
                            ::std::result::Result::Ok(())
                        })?;
                    }
                }
            }
        }
    }

    /// Generate code for a pragma command.
    fn gen_pragma(
        &mut self,
        kind: &IrValue,
        targets: &[IrValue],
        em_ident: &proc_macro2::Ident,
    ) -> TokenStream {
        // Optimize known literal kinds to direct emitter methods
        if let IrValue::Literal(k) = kind {
            match k.as_str() {
                "claim" | "unclaim" | "observe" | "unobserve" | "handle" | "unhandle" | "tap"
                | "untap" => {
                    let method = format_ident!("{k}");
                    let many_method = format_ident!("{k}_many");

                    if targets.len() == 1 {
                        let (target_binds, target_expr) = self.gen_str_value(&targets[0]);
                        return quote! {
                            #target_binds
                            #em_ident.#method(#target_expr)?;
                        };
                    } else {
                        // Multiple targets — build a slice
                        let target_items: Vec<TokenStream> = targets
                            .iter()
                            .map(|t| match t {
                                IrValue::Literal(s) => quote! { #s },
                                IrValue::Interpolation(expr) => {
                                    quote! { (#expr).as_ref() }
                                }
                            })
                            .collect();
                        return quote! {
                            #em_ident.#many_method(&[#(#target_items),*])?;
                        };
                    }
                }
                "redirect" => {
                    let (target_binds, target_expr) =
                        self.gen_str_value(targets.first().expect("redirect needs a target"));
                    return quote! {
                        #target_binds
                        #em_ident.redirect(#target_expr)?;
                    };
                }
                "unredirect" => {
                    return quote! {
                        #em_ident.unredirect()?;
                    };
                }
                _ => {}
            }
        }

        // Generic pragma: fall back to writing raw
        let (kind_binds, kind_expr) = self.gen_str_value(kind);
        if targets.is_empty() {
            quote! {
                #kind_binds
                write!(#em_ident.writer, "\n#{}", #kind_expr)?;
            }
        } else {
            let target_items: Vec<TokenStream> = targets
                .iter()
                .map(|t| match t {
                    IrValue::Literal(s) => quote! { #s },
                    IrValue::Interpolation(expr) => {
                        quote! { (#expr).as_ref() }
                    }
                })
                .collect();
            quote! {
                #kind_binds
                write!(#em_ident.writer, "\n#{} {}", #kind_expr, [#(#target_items),*].join(","))?;
            }
        }
    }

    /// Generate code for a request command.
    fn gen_request(
        &mut self,
        kind: &IrValue,
        seq: &IrValue,
        targets: &[IrValue],
        props: &[IrProp],
        em_ident: &proc_macro2::Ident,
    ) -> TokenStream {
        let (seq_binds, seq_expr) = self.gen_seq_value(seq);

        // Optimize known literal kinds to direct emitter methods
        if let IrValue::Literal(k) = kind
            && k.as_str() == "expand"
        {
            let (target_binds, target_expr) =
                self.gen_str_value(targets.first().expect("expand needs a target"));
            if !props.is_empty() {
                if has_dynamic_props(props) {
                    let props_var = self.fresh_ident("props");
                    let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                    return quote! {
                        {
                            #seq_binds
                            #target_binds
                            let mut #props_var = ::std::vec::Vec::new();
                            #push_stmts
                            #em_ident.expand(#seq_expr, #target_expr, &#props_var)?;
                        }
                    };
                } else {
                    let prop_items = self.gen_static_prop_items(props);
                    return quote! {
                        #seq_binds
                        #target_binds
                        #em_ident.expand(#seq_expr, #target_expr, &[#(#prop_items),*])?;
                    };
                }
            } else {
                return quote! {
                    #seq_binds
                    #target_binds
                    #em_ident.expand(#seq_expr, #target_expr, &[])?;
                };
            }
        }

        // Generic request: ?kind seq target props...
        let (kind_binds, kind_expr) = self.gen_str_value(kind);
        let (target_binds, target_expr) =
            self.gen_str_value(targets.first().expect("request needs a target"));
        if has_dynamic_props(props) {
            let props_var = self.fresh_ident("props");
            let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
            quote! {
                {
                    #kind_binds
                    #seq_binds
                    #target_binds
                    let mut #props_var = ::std::vec::Vec::new();
                    #push_stmts
                    #em_ident.request(#kind_expr, #seq_expr, #target_expr, &#props_var)?;
                }
            }
        } else {
            let prop_items = self.gen_static_prop_items(props);
            quote! {
                #kind_binds
                #seq_binds
                #target_binds
                #em_ident.request(#kind_expr, #seq_expr, #target_expr, &[#(#prop_items),*])?;
            }
        }
    }

    /// Generate code for a response command.
    fn gen_response(
        &mut self,
        kind: &IrValue,
        seq: &IrValue,
        props: &[IrProp],
        children: &Option<Vec<IrCommand>>,
        em_ident: &proc_macro2::Ident,
    ) -> TokenStream {
        let (kind_binds, kind_expr) = self.gen_str_value(kind);
        let (seq_binds, seq_expr) = self.gen_seq_value(seq);

        // .expand → expanded_with (closure-based)
        if let IrValue::Literal(k) = kind
            && k == "expand"
            && let Some(ch) = children
        {
            let child_code = self.gen_commands(ch, em_ident);
            return quote! {
                #seq_binds
                #em_ident.expanded_with(#seq_expr, |#em_ident| {
                    #child_code
                    ::std::result::Result::Ok(())
                })?;
            };
        }

        match children {
            Some(ch) => {
                let child_code = self.gen_commands(ch, em_ident);
                if has_dynamic_props(props) {
                    let props_var = self.fresh_ident("props");
                    let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                    quote! {
                        {
                            #kind_binds
                            #seq_binds
                            let mut #props_var = ::std::vec::Vec::new();
                            #push_stmts
                            #em_ident.response_with(#kind_expr, #seq_expr, &#props_var, |#em_ident| {
                                #child_code
                                ::std::result::Result::Ok(())
                            })?;
                        }
                    }
                } else {
                    let prop_items = self.gen_static_prop_items(props);
                    quote! {
                        #kind_binds
                        #seq_binds
                        #em_ident.response_with(#kind_expr, #seq_expr, &[#(#prop_items),*], |#em_ident| {
                            #child_code
                            ::std::result::Result::Ok(())
                        })?;
                    }
                }
            }
            None => {
                if has_dynamic_props(props) {
                    let props_var = self.fresh_ident("props");
                    let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                    quote! {
                        {
                            #kind_binds
                            #seq_binds
                            let mut #props_var = ::std::vec::Vec::new();
                            #push_stmts
                            #em_ident.response(#kind_expr, #seq_expr, &#props_var)?;
                        }
                    }
                } else {
                    let prop_items = self.gen_static_prop_items(props);
                    quote! {
                        #kind_binds
                        #seq_binds
                        #em_ident.response(#kind_expr, #seq_expr, &[#(#prop_items),*])?;
                    }
                }
            }
        }
    }

    fn gen_message(
        &mut self,
        kind: &IrValue,
        target: &IrValue,
        props: &[IrProp],
        children: &Option<Vec<IrCommand>>,
        em_ident: &proc_macro2::Ident,
    ) -> TokenStream {
        let (kind_binds, kind_expr) = self.gen_str_value(kind);
        let (target_binds, target_expr) = self.gen_str_value(target);

        match children {
            Some(ch) => {
                let child_code = self.gen_commands(ch, em_ident);
                if has_dynamic_props(props) {
                    let props_var = self.fresh_ident("props");
                    let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                    quote! {
                        {
                            #kind_binds
                            #target_binds
                            let mut #props_var = ::std::vec::Vec::new();
                            #push_stmts
                            #em_ident.message_with(#kind_expr, #target_expr, &#props_var, |#em_ident| {
                                #child_code
                                ::std::result::Result::Ok(())
                            })?;
                        }
                    }
                } else {
                    let prop_items = self.gen_static_prop_items(props);
                    quote! {
                        #kind_binds
                        #target_binds
                        #em_ident.message_with(#kind_expr, #target_expr, &[#(#prop_items),*], |#em_ident| {
                            #child_code
                            ::std::result::Result::Ok(())
                        })?;
                    }
                }
            }
            None => {
                if has_dynamic_props(props) {
                    let props_var = self.fresh_ident("props");
                    let push_stmts = self.gen_dynamic_prop_pushes(props, &props_var);
                    quote! {
                        {
                            #kind_binds
                            #target_binds
                            let mut #props_var = ::std::vec::Vec::new();
                            #push_stmts
                            #em_ident.message(#kind_expr, #target_expr, &#props_var)?;
                        }
                    }
                } else {
                    let prop_items = self.gen_static_prop_items(props);
                    quote! {
                        #kind_binds
                        #target_binds
                        #em_ident.message(#kind_expr, #target_expr, &[#(#prop_items),*])?;
                    }
                }
            }
        }
    }

    /// For a string-position IrValue, return (optional bindings, expression to use as &str).
    fn gen_str_value(&mut self, val: &IrValue) -> (TokenStream, TokenStream) {
        match val {
            IrValue::Literal(s) => (quote! {}, quote! { #s }),
            IrValue::Interpolation(expr) => {
                let v = self.fresh_ident("v");
                (quote! { let #v = #expr; }, quote! { #v.as_ref() })
            }
        }
    }

    /// For a sequence number IrValue, return (optional bindings, expression to use as u64).
    fn gen_seq_value(&mut self, val: &IrValue) -> (TokenStream, TokenStream) {
        match val {
            IrValue::Literal(s) => {
                // Parse as u64 at compile time
                let n: u64 = s.parse().expect("sequence number must be a valid u64");
                (quote! {}, quote! { #n })
            }
            IrValue::Interpolation(expr) => (quote! {}, quote! { #expr }),
        }
    }

    /// Generate static prop items for a `&[Prop::..., ...]` slice literal.
    /// Only called when `has_dynamic_props` returned false.
    fn gen_static_prop_items(&self, props: &[IrProp]) -> Vec<TokenStream> {
        props
            .iter()
            .map(|p| match p {
                IrProp::Value {
                    key,
                    value: IrValue::Literal(v),
                } => {
                    quote! { ::byo::Prop::val(#key, #v) }
                }
                IrProp::Boolean { key } => {
                    quote! { ::byo::Prop::flag(#key) }
                }
                IrProp::Remove { key } => {
                    quote! { ::byo::Prop::remove(#key) }
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
                quote! { #vec_ident.push(::byo::Prop::val(#key, #v)); }
            }
            IrProp::Value {
                key,
                value: IrValue::Interpolation(expr),
            } => {
                quote! { #vec_ident.push(::byo::Prop::val(#key, #expr)); }
            }
            IrProp::Boolean { key } => {
                quote! { #vec_ident.push(::byo::Prop::flag(#key)); }
            }
            IrProp::Remove { key } => {
                quote! { #vec_ident.push(::byo::Prop::remove(#key)); }
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

/// Generate code for `byo_write!(em, { ... })`.
/// Returns a `TokenStream` that evaluates to `io::Result<()>`.
pub fn gen_byo_write(em_expr: TokenStream, cmds: &[IrCommand]) -> TokenStream {
    let em_ident = format_ident!("__em");
    let mut cg = Codegen::new();
    let body = cg.gen_commands(cmds, &em_ident);
    quote! {
        (|#em_ident: &mut ::byo::emitter::Emitter<_>| -> ::std::io::Result<()> {
            #body
            ::std::result::Result::Ok(())
        })(#em_expr)
    }
}

/// Generate code for `byo! { ... }`.
/// Returns a `TokenStream` that writes a framed APC batch to stdout.
pub fn gen_byo(cmds: &[IrCommand]) -> TokenStream {
    let em_ident = format_ident!("__em");
    let mut cg = Codegen::new();
    let body = cg.gen_commands(cmds, &em_ident);
    quote! {
        {
            let __stdout = ::std::io::stdout();
            let mut __lock = __stdout.lock();
            let mut #em_ident = ::byo::emitter::Emitter::new(&mut __lock);
            #em_ident.frame(|#em_ident| {
                #body
                ::std::result::Result::Ok(())
            }).expect("byo!: write to stdout failed");
        }
    }
}
