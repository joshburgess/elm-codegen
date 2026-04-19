use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Attribute, Data, DeriveInput, Fields, Lit, Type, TypePath};

/// Derive macro that generates `ElmType` for a struct.
///
/// # Container attributes
///
/// - `#[elm(module = "Api.Person")]`: Elm module path (required)
/// - `#[elm(name = "Person")]`: Elm type name (required)
/// - `#[elm(tags = "entity,response")]`: comma-separated free-form tags
///   that downstream tooling can branch on (optional)
///
/// # Field attributes
///
/// - `#[elm(skip)]`: exclude this field from the Elm type
/// - `#[elm(type = "CustomElmType")]`: override the inferred Elm type
/// - `#[elm(name = "customName")]`: override the camelCase field name
/// - `#[elm(decoder = "customDecoder")]`: substitute a decoder expression
///
/// `#[serde(rename = "...")]` and `#[serde(rename_all = "...")]` are
/// honored when computing the JSON key.
#[proc_macro_derive(ElmType, attributes(elm, serde))]
pub fn derive_elm_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_elm_type(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn impl_elm_type(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    let container = parse_container_attrs(&input.attrs)?;
    let module_str = container.module.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(name, "missing #[elm(module = \"...\")] attribute")
    })?;
    let type_name_str = container.type_name.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(name, "missing #[elm(name = \"...\")] attribute")
    })?;

    let serde_rename_all = parse_serde_rename_all(&input.attrs);

    let module_segments: Vec<&str> = module_str.split('.').collect();
    let module_tokens = quote! { vec![#(#module_segments),*] };

    let tag_strs: Vec<&str> = container
        .tags
        .as_deref()
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let tags_tokens = quote! { vec![#(#tag_strs),*] };

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "ElmType can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "ElmType can only be derived for structs",
            ));
        }
    };

    let mut field_tokens = Vec::new();

    for field in fields {
        let field_attrs = parse_field_attrs(&field.attrs)?;
        if field_attrs.skip {
            continue;
        }

        let Some(field_name) = field.ident.as_ref() else {
            return Err(syn::Error::new_spanned(
                field,
                "ElmType requires named fields",
            ));
        };
        let rust_name_str = field_name.to_string();

        let json_key = if let Some(ref rename) = parse_serde_field_rename(&field.attrs) {
            rename.clone()
        } else if let Some(ref rename_all) = serde_rename_all {
            apply_rename_all(&rust_name_str, rename_all)
        } else {
            rust_name_str.clone()
        };

        let elm_name = if let Some(ref custom_name) = field_attrs.name {
            custom_name.clone()
        } else {
            snake_to_camel(&rust_name_str)
        };

        let (type_repr_tokens, is_optional) = if let Some(ref custom_type) = field_attrs.type_override {
            (
                quote! { elm_codegen_core::ElmTypeRepr::Custom(#custom_type.to_string()) },
                false,
            )
        } else {
            rust_type_to_elm_repr(&field.ty)?
        };

        let decoder_tokens = match &field_attrs.decoder {
            Some(d) => quote! { Some(#d) },
            None => quote! { None },
        };

        field_tokens.push(quote! {
            elm_codegen_core::ElmFieldInfo {
                rust_name: #json_key,
                elm_name: #elm_name,
                elm_type: #type_repr_tokens,
                is_optional: #is_optional,
                custom_decoder: #decoder_tokens,
            }
        });
    }

    let rust_name_str = name.to_string();
    let expanded = quote! {
        impl elm_codegen_core::ElmType for #name {
            fn elm_type_info() -> elm_codegen_core::ElmTypeInfo {
                elm_codegen_core::ElmTypeInfo {
                    rust_name: #rust_name_str,
                    module_path: #module_tokens,
                    type_name: #type_name_str,
                    tags: #tags_tokens,
                    fields: vec![#(#field_tokens),*],
                }
            }
        }

        elm_codegen_core::inventory::submit! {
            elm_codegen_core::ElmTypeRegistration {
                build: <#name as elm_codegen_core::ElmType>::elm_type_info,
            }
        }
    };

    Ok(expanded)
}

// ── Container attribute parsing ─────────────────────────────────────

struct ContainerAttrs {
    module: Option<String>,
    type_name: Option<String>,
    tags: Option<String>,
}

fn parse_container_attrs(attrs: &[Attribute]) -> syn::Result<ContainerAttrs> {
    let mut result = ContainerAttrs {
        module: None,
        type_name: None,
        tags: None,
    };

    for attr in attrs {
        if !attr.path().is_ident("elm") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("module") {
                result.module = Some(parse_str_value(&meta)?);
                Ok(())
            } else if meta.path.is_ident("name") {
                result.type_name = Some(parse_str_value(&meta)?);
                Ok(())
            } else if meta.path.is_ident("tags") {
                result.tags = Some(parse_str_value(&meta)?);
                Ok(())
            } else {
                Err(meta.error("unknown elm container attribute"))
            }
        })?;
    }

    Ok(result)
}

// ── Field attribute parsing ─────────────────────────────────────────

struct FieldAttrs {
    skip: bool,
    type_override: Option<String>,
    name: Option<String>,
    decoder: Option<String>,
}

fn parse_field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut result = FieldAttrs {
        skip: false,
        type_override: None,
        name: None,
        decoder: None,
    };

    for attr in attrs {
        if !attr.path().is_ident("elm") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                result.skip = true;
                Ok(())
            } else if meta.path.is_ident("type") {
                result.type_override = Some(parse_str_value(&meta)?);
                Ok(())
            } else if meta.path.is_ident("name") {
                result.name = Some(parse_str_value(&meta)?);
                Ok(())
            } else if meta.path.is_ident("decoder") {
                result.decoder = Some(parse_str_value(&meta)?);
                Ok(())
            } else {
                Err(meta.error("unknown elm field attribute"))
            }
        })?;
    }

    Ok(result)
}

fn parse_str_value(meta: &syn::meta::ParseNestedMeta) -> syn::Result<String> {
    let value = meta.value()?;
    let lit: Lit = value.parse()?;
    if let Lit::Str(s) = lit {
        Ok(s.value())
    } else {
        Err(meta.error("expected a string literal"))
    }
}

// ── Serde attribute parsing ─────────────────────────────────────────

fn parse_serde_rename_all(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut rename_all = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    rename_all = Some(s.value());
                }
            }
            Ok(())
        });
        if rename_all.is_some() {
            return rename_all;
        }
    }
    None
}

fn parse_serde_field_rename(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut rename = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    rename = Some(s.value());
                }
            }
            Ok(())
        });
        if rename.is_some() {
            return rename;
        }
    }
    None
}

fn apply_rename_all(field_name: &str, convention: &str) -> String {
    match convention {
        "camelCase" => snake_to_camel(field_name),
        "snake_case" => field_name.to_string(),
        "SCREAMING_SNAKE_CASE" => field_name.to_uppercase(),
        "kebab-case" => field_name.replace('_', "-"),
        _ => field_name.to_string(),
    }
}

// ── Type mapping ────────────────────────────────────────────────────

fn rust_type_to_elm_repr(ty: &Type) -> syn::Result<(proc_macro2::TokenStream, bool)> {
    match ty {
        Type::Path(type_path) => path_to_elm_repr(type_path),
        _ => Ok((
            quote! { elm_codegen_core::ElmTypeRepr::Custom("Unknown".to_string()) },
            false,
        )),
    }
}

fn path_to_elm_repr(type_path: &TypePath) -> syn::Result<(proc_macro2::TokenStream, bool)> {
    let Some(segment) = type_path.path.segments.last() else {
        return Err(syn::Error::new_spanned(type_path, "empty type path"));
    };
    let ident = segment.ident.to_string();

    match ident.as_str() {
        "String" => Ok((quote! { elm_codegen_core::ElmTypeRepr::String }, false)),
        "bool" => Ok((quote! { elm_codegen_core::ElmTypeRepr::Bool }, false)),
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize" => {
            Ok((quote! { elm_codegen_core::ElmTypeRepr::Int }, false))
        }
        "f32" | "f64" => Ok((quote! { elm_codegen_core::ElmTypeRepr::Float }, false)),
        "DateTime" => Ok((quote! { elm_codegen_core::ElmTypeRepr::Posix }, false)),
        "NaiveDate" => Ok((quote! { elm_codegen_core::ElmTypeRepr::IsoDate }, false)),
        "Option" => {
            let inner = extract_single_generic_arg(segment)?;
            let (inner_tokens, _) = rust_type_to_elm_repr(&inner)?;
            Ok((
                quote! { elm_codegen_core::ElmTypeRepr::Maybe(Box::new(#inner_tokens)) },
                true,
            ))
        }
        "Vec" => {
            let inner = extract_single_generic_arg(segment)?;
            let (inner_tokens, _) = rust_type_to_elm_repr(&inner)?;
            Ok((
                quote! { elm_codegen_core::ElmTypeRepr::List(Box::new(#inner_tokens)) },
                false,
            ))
        }
        other => Ok((
            quote! { elm_codegen_core::ElmTypeRepr::Custom(#other.to_string()) },
            false,
        )),
    }
}

fn extract_single_generic_arg(segment: &syn::PathSegment) -> syn::Result<Type> {
    match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => {
            for arg in &args.args {
                if let syn::GenericArgument::Type(ty) = arg {
                    return Ok(ty.clone());
                }
            }
            Err(syn::Error::new_spanned(
                segment,
                "expected a type argument",
            ))
        }
        _ => Err(syn::Error::new_spanned(
            segment,
            "expected angle-bracketed arguments",
        )),
    }
}

fn snake_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}
