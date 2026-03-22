use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericArgument, PathArguments, Type, parse_macro_input};

/// Derive macro that generates a `FromLuaTable` implementation for a struct.
///
/// Each field is converted from the corresponding key in a
/// `HashMap<String, LuaTableValue>`. Four shapes are supported:
///
/// - **Primitive** (`String`, `i64`, `f64`, `bool`) — required; returns `Err`
///   if the key is absent or the variant doesn't match.
/// - **`Option<T>`** — absent key → `None`; wrong variant → `Err`.
/// - **`Vec<T>`** — expects `LuaTableValue::List`; each element is converted
///   recursively with the same rules as a primitive or nested struct.
/// - **Nested struct** — any type not recognised as a primitive and not wrapped
///   in `Option`/`Vec` is assumed to implement `FromLuaTable` itself; it is
///   extracted from `LuaTableValue::Map`.
///
/// Generated code references types via `lua_table::*` — neither the engine nor
/// this crate depend on `lua_script_manager` or `mlua`.
#[proc_macro_derive(FromLuaTable)]
pub fn from_lua_table(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

// ─── Top-level expansion ─────────────────────────────────────────────────────

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "FromLuaTable can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "FromLuaTable can only be derived for structs",
            ));
        }
    };

    let field_conversions: Vec<TokenStream2> = fields
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let key = ident.to_string();
            field_conversion(ident, &key, &f.ty)
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let field_idents: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();

    Ok(quote! {
        impl lua_table::FromLuaTable for #name {
            fn from_lua_table(
                mut map: std::collections::HashMap<String, lua_table::LuaTableValue>
            ) -> Result<Self, String> {
                #(#field_conversions)*
                Ok(Self { #(#field_idents),* })
            }
        }
    })
}

// ─── Per-field code generation ────────────────────────────────────────────────

fn field_conversion(ident: &syn::Ident, key: &str, ty: &Type) -> syn::Result<TokenStream2> {
    if let Some(inner) = unwrap_option(ty) {
        let extract = extract_value(inner, key, true)?;
        Ok(quote! { let #ident = #extract; })
    } else if let Some(elem) = unwrap_vec(ty) {
        let extract = extract_list(elem, key)?;
        Ok(quote! { let #ident = #extract; })
    } else {
        let extract = extract_value(ty, key, false)?;
        Ok(quote! { let #ident = #extract; })
    }
}

fn extract_value(ty: &Type, key: &str, optional: bool) -> syn::Result<TokenStream2> {
    let variant_match = primitive_variant_match(ty);

    if optional {
        if let Some(variant_arm) = variant_match {
            Ok(quote! {
                match map.remove(#key) {
                    None => None,
                    Some(lua_table::LuaTableValue::#variant_arm) => Some(v),
                    Some(other) => return Err(format!(
                        concat!("field `", #key, "`: expected ", stringify!(#variant_arm),
                                ", got {:?}"),
                        other
                    )),
                }
            })
        } else {
            Ok(quote! {
                match map.remove(#key) {
                    None => None,
                    Some(lua_table::LuaTableValue::Map(inner)) => {
                        Some(<#ty as lua_table::FromLuaTable>::from_lua_table(inner)
                            .map_err(|e| format!(concat!("field `", #key, "`: {}"), e))?)
                    }
                    Some(other) => return Err(format!(
                        concat!("field `", #key, "`: expected Map for nested struct, got {:?}"),
                        other
                    )),
                }
            })
        }
    } else if let Some(variant_arm) = variant_match {
        Ok(quote! {
            match map.remove(#key) {
                Some(lua_table::LuaTableValue::#variant_arm) => v,
                Some(other) => return Err(format!(
                    concat!("field `", #key, "`: expected ", stringify!(#variant_arm),
                            ", got {:?}"),
                    other
                )),
                None => return Err(format!(concat!("field `", #key, "` is missing"))),
            }
        })
    } else {
        Ok(quote! {
            match map.remove(#key) {
                Some(lua_table::LuaTableValue::Map(inner)) => {
                    <#ty as lua_table::FromLuaTable>::from_lua_table(inner)
                        .map_err(|e| format!(concat!("field `", #key, "`: {}"), e))?
                }
                Some(other) => return Err(format!(
                    concat!("field `", #key, "`: expected Map for nested struct, got {:?}"),
                    other
                )),
                None => return Err(format!(concat!("field `", #key, "` is missing"))),
            }
        })
    }
}

fn extract_list(elem_ty: &Type, key: &str) -> syn::Result<TokenStream2> {
    let elem_variant = primitive_variant_match(elem_ty);

    let elem_conversion = if let Some(variant_arm) = elem_variant {
        quote! {
            match item {
                lua_table::LuaTableValue::#variant_arm => v,
                other => return Err(format!(
                    concat!("field `", #key, "`: list element: expected ",
                            stringify!(#variant_arm), ", got {:?}"),
                    other
                )),
            }
        }
    } else {
        quote! {
            match item {
                lua_table::LuaTableValue::Map(inner) => {
                    <#elem_ty as lua_table::FromLuaTable>::from_lua_table(inner)
                        .map_err(|e| format!(concat!("field `", #key, "`: list element: {}"), e))?
                }
                other => return Err(format!(
                    concat!("field `", #key, "`: list element: expected Map, got {:?}"),
                    other
                )),
            }
        }
    };

    Ok(quote! {
        match map.remove(#key) {
            None => return Err(format!(concat!("field `", #key, "` is missing"))),
            Some(lua_table::LuaTableValue::List(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(#elem_conversion);
                }
                out
            }
            Some(other) => return Err(format!(
                concat!("field `", #key, "`: expected List, got {:?}"),
                other
            )),
        }
    })
}

// ─── Type introspection helpers ───────────────────────────────────────────────

fn unwrap_option(ty: &Type) -> Option<&Type> {
    let seg = single_path_segment(ty)?;
    if seg.ident != "Option" {
        return None;
    }
    first_generic_type_arg(&seg.arguments)
}

fn unwrap_vec(ty: &Type) -> Option<&Type> {
    let seg = single_path_segment(ty)?;
    if seg.ident != "Vec" {
        return None;
    }
    first_generic_type_arg(&seg.arguments)
}

fn single_path_segment(ty: &Type) -> Option<&syn::PathSegment> {
    if let Type::Path(tp) = ty {
        if tp.qself.is_none() {
            return tp.path.segments.last();
        }
    }
    None
}

fn first_generic_type_arg(args: &PathArguments) -> Option<&Type> {
    if let PathArguments::AngleBracketed(ab) = args {
        for arg in &ab.args {
            if let GenericArgument::Type(t) = arg {
                return Some(t);
            }
        }
    }
    None
}

fn primitive_variant_match(ty: &Type) -> Option<TokenStream2> {
    let seg = single_path_segment(ty)?;
    match seg.ident.to_string().as_str() {
        "String" => Some(quote! { String(v) }),
        "i64" => Some(quote! { Int(v) }),
        "f64" => Some(quote! { Float(v) }),
        "bool" => Some(quote! { Bool(v) }),
        _ => None,
    }
}
