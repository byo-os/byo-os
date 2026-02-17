//! Derive macro implementations for `FromProps`, `ToProps`, `ReadProp`, and `WriteProp`.
//!
//! **Struct-level** (`FromProps` / `ToProps`):
//! Generates trait impls that dispatch to per-field `ReadProp`/`WriteProp` calls.
//! Field names are auto-converted from snake_case to kebab-case on the wire.
//!
//! **Enum-level** (`ReadProp` / `WriteProp`):
//! For unit enums only. Variant names are auto-converted from PascalCase to
//! kebab-case on the wire.
//!
//! # Attributes
//!
//! - `#[prop(rename = "wire-name")]` — custom wire key (overrides auto-conversion)
//! - `#[prop(skip)]` — exclude field, use `Default::default()` (structs only)

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr};

// ---------------------------------------------------------------------------
// Case conversion (compile-time only, runs in proc macro)
// ---------------------------------------------------------------------------

/// Convert `snake_case` to `kebab-case`: replace `_` with `-`.
fn snake_to_kebab(s: &str) -> String {
    s.replace('_', "-")
}

/// Convert `PascalCase` to `kebab-case`.
///
/// - `Primary` → `primary`
/// - `PrimaryOutline` → `primary-outline`
/// - `BGColor` → `bg-color`
/// - `HTMLParser` → `html-parser`
fn pascal_to_kebab(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
                if prev.is_lowercase() || (prev.is_uppercase() && next_is_lower) {
                    result.push('-');
                }
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Attribute parsing helpers
// ---------------------------------------------------------------------------

struct PropAttr {
    rename: Option<String>,
    skip: bool,
}

fn parse_prop_attr(attrs: &[syn::Attribute]) -> Result<PropAttr, TokenStream> {
    let mut rename = None;
    let mut skip = false;

    for attr in attrs {
        if !attr.path().is_ident("prop") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                skip = true;
                return Ok(());
            }
            if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let lit: LitStr = value.parse()?;
                rename = Some(lit.value());
                return Ok(());
            }
            Err(meta.error("expected `skip` or `rename = \"...\"`"))
        })
        .map_err(|e| e.to_compile_error())?;
    }

    Ok(PropAttr { rename, skip })
}

// ---------------------------------------------------------------------------
// Struct derives: FromProps / ToProps
// ---------------------------------------------------------------------------

struct FieldInfo {
    ident: syn::Ident,
    ty: syn::Type,
    wire_key: String,
    skip: bool,
}

fn parse_fields(input: &DeriveInput) -> Result<Vec<FieldInfo>, TokenStream> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "FromProps/ToProps requires named fields",
                )
                .to_compile_error());
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "FromProps/ToProps can only be derived on structs",
            )
            .to_compile_error());
        }
    };

    let mut infos = Vec::new();
    for field in fields {
        let ident = field.ident.clone().unwrap();
        let ty = field.ty.clone();
        let attr = parse_prop_attr(&field.attrs)?;
        let wire_key = attr
            .rename
            .unwrap_or_else(|| snake_to_kebab(&ident.to_string()));

        infos.push(FieldInfo {
            ident,
            ty,
            wire_key,
            skip: attr.skip,
        });
    }
    Ok(infos)
}

pub fn derive_from_props(input: DeriveInput) -> TokenStream {
    let fields = match parse_fields(&input) {
        Ok(f) => f,
        Err(e) => return e,
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let field_defaults: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let ty = &f.ty;
            quote! { let mut #ident = <#ty as ::std::default::Default>::default(); }
        })
        .collect();

    let match_arms: Vec<TokenStream> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| {
            let ident = &f.ident;
            let wire_key = &f.wire_key;
            quote! { #wire_key => ::byo::props::ReadProp::apply(&mut #ident, __prop), }
        })
        .collect();

    let field_inits: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            quote! { #ident, }
        })
        .collect();

    quote! {
        impl #impl_generics ::byo::props::FromProps for #name #ty_generics #where_clause {
            fn from_props(__props: &[::byo::Prop]) -> Self {
                #(#field_defaults)*
                for __prop in __props {
                    match __prop.key() {
                        #(#match_arms)*
                        _ => {}
                    }
                }
                Self { #(#field_inits)* }
            }
        }
    }
}

pub fn derive_to_props(input: DeriveInput) -> TokenStream {
    let fields = match parse_fields(&input) {
        Ok(f) => f,
        Err(e) => return e,
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let encode_stmts: Vec<TokenStream> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| {
            let ident = &f.ident;
            let wire_key = &f.wire_key;
            quote! { ::byo::props::WriteProp::encode(&self.#ident, #wire_key, &mut __props); }
        })
        .collect();

    quote! {
        impl #impl_generics ::byo::props::ToProps for #name #ty_generics #where_clause {
            fn to_props(&self) -> ::std::vec::Vec<::byo::Prop> {
                let mut __props = ::std::vec::Vec::new();
                #(#encode_stmts)*
                __props
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Enum derives: ReadProp / WriteProp
// ---------------------------------------------------------------------------

struct VariantInfo {
    ident: syn::Ident,
    wire_name: String,
}

fn parse_unit_variants(input: &DeriveInput) -> Result<Vec<VariantInfo>, TokenStream> {
    let variants = match &input.data {
        Data::Enum(e) => &e.variants,
        _ => unreachable!("caller checks for enum"),
    };

    let mut infos = Vec::new();
    for variant in variants {
        if !variant.fields.is_empty() {
            return Err(syn::Error::new_spanned(
                variant,
                "ReadProp/WriteProp derive only supports unit variants (no fields)",
            )
            .to_compile_error());
        }
        let attr = parse_prop_attr(&variant.attrs)?;
        let wire_name = attr
            .rename
            .unwrap_or_else(|| pascal_to_kebab(&variant.ident.to_string()));

        infos.push(VariantInfo {
            ident: variant.ident.clone(),
            wire_name,
        });
    }
    Ok(infos)
}

/// Returns the inner type if this is a newtype (single unnamed field).
fn newtype_inner(input: &DeriveInput) -> Option<&syn::Type> {
    match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Unnamed(f) if f.unnamed.len() == 1 => Some(&f.unnamed[0].ty),
            _ => None,
        },
        _ => None,
    }
}

pub fn derive_read_prop(input: DeriveInput) -> TokenStream {
    if let Some(inner_ty) = newtype_inner(&input) {
        return derive_read_prop_newtype(&input, inner_ty);
    }
    if matches!(&input.data, Data::Enum(_)) {
        return derive_read_prop_enum(&input);
    }
    syn::Error::new_spanned(
        &input,
        "ReadProp can only be derived on unit enums or newtype structs",
    )
    .to_compile_error()
}

pub fn derive_write_prop(input: DeriveInput) -> TokenStream {
    if let Some(inner_ty) = newtype_inner(&input) {
        return derive_write_prop_newtype(&input, inner_ty);
    }
    if matches!(&input.data, Data::Enum(_)) {
        return derive_write_prop_enum(&input);
    }
    syn::Error::new_spanned(
        &input,
        "WriteProp can only be derived on unit enums or newtype structs",
    )
    .to_compile_error()
}

// --- Enum impls ---

fn derive_read_prop_enum(input: &DeriveInput) -> TokenStream {
    let variants = match parse_unit_variants(input) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let match_arms: Vec<TokenStream> = variants
        .iter()
        .map(|v| {
            let ident = &v.ident;
            let wire_name = &v.wire_name;
            quote! { #wire_name => *self = Self::#ident, }
        })
        .collect();

    quote! {
        impl #impl_generics ::byo::props::ReadProp for #name #ty_generics #where_clause {
            fn apply(&mut self, __prop: &::byo::Prop) {
                if let ::byo::Prop::Value { value, .. } = __prop {
                    match value.as_ref() {
                        #(#match_arms)*
                        _ => {}
                    }
                }
            }
        }
    }
}

fn derive_write_prop_enum(input: &DeriveInput) -> TokenStream {
    let variants = match parse_unit_variants(input) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let match_arms: Vec<TokenStream> = variants
        .iter()
        .map(|v| {
            let ident = &v.ident;
            let wire_name = &v.wire_name;
            quote! { Self::#ident => #wire_name, }
        })
        .collect();

    quote! {
        impl #impl_generics ::byo::props::WriteProp for #name #ty_generics #where_clause {
            fn encode(&self, __key: &str, __out: &mut ::std::vec::Vec<::byo::Prop>) {
                let __val = match self {
                    #(#match_arms)*
                };
                __out.push(::byo::Prop::val(__key, __val));
            }
        }
    }
}

// --- Newtype impls ---

fn derive_read_prop_newtype(input: &DeriveInput, _inner_ty: &syn::Type) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::byo::props::ReadProp for #name #ty_generics #where_clause {
            fn apply(&mut self, __prop: &::byo::Prop) {
                ::byo::props::ReadProp::apply(&mut self.0, __prop);
            }
        }
    }
}

fn derive_write_prop_newtype(input: &DeriveInput, _inner_ty: &syn::Type) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::byo::props::WriteProp for #name #ty_generics #where_clause {
            fn encode(&self, __key: &str, __out: &mut ::std::vec::Vec<::byo::Prop>) {
                ::byo::props::WriteProp::encode(&self.0, __key, __out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_to_kebab_basic() {
        assert_eq!(snake_to_kebab("data_value"), "data-value");
        assert_eq!(snake_to_kebab("bg_color"), "bg-color");
        assert_eq!(snake_to_kebab("class"), "class");
        assert_eq!(snake_to_kebab("order"), "order");
    }

    #[test]
    fn pascal_to_kebab_basic() {
        assert_eq!(pascal_to_kebab("Primary"), "primary");
        assert_eq!(pascal_to_kebab("PrimaryOutline"), "primary-outline");
        assert_eq!(pascal_to_kebab("BGColor"), "bg-color");
        assert_eq!(pascal_to_kebab("HTMLParser"), "html-parser");
        assert_eq!(pascal_to_kebab("A"), "a");
    }
}
