use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Attribute, Data, DeriveInput, Fields, FnArg, Ident, ItemFn, Lit, LitStr,
    ReturnType, Token, Type, TypePath,
};

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
/// - `#[elm(encoder = "customEncoder")]`: substitute an encoder function;
///   applied as `customEncoder value.fieldName` inside `Encode.object`
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

    // Reject generic types up front. The Elm side has nothing to
    // monomorphize against (the registry collects the type once at
    // link time), so a generic Rust struct can't produce a sensible
    // single Elm declaration. The usual answer is to make a concrete
    // wrapper struct per instantiation, or to override with
    // `#[elm(type = "...")]` at every use site.
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "ElmType can't be derived on a generic type. The registry materializes one Elm declaration per Rust type, with no way to monomorphize against caller arguments. Pick one of: define a concrete wrapper struct per instantiation (e.g. `pub struct PageOfPersonApi(Page<PersonApi>)` plus `#[derive(ElmType)]` on the wrapper), or stop deriving and write the Elm types by hand for this case",
        ));
    }

    let container = parse_container_attrs(&input.attrs)?;
    let module_str = container.module.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(name, "missing #[elm(module = \"...\")] attribute")
    })?;
    // `name` defaults to the Rust ident's string, which lets macro-generated
    // structs (e.g. ones created by a `macro_rules!`) skip naming the type
    // explicitly while still producing a usable Elm type name.
    let rust_ident_string = name.to_string();
    let type_name_str = container.type_name.as_ref().unwrap_or(&rust_ident_string);

    let serde_rename_all = parse_serde_rename_all(&input.attrs);

    let module_segments: Vec<&str> = module_str.split('.').collect();
    let module_tokens = quote! { vec![#(#module_segments),*] };

    let tag_strs: Vec<&str> = container
        .tags
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let tags_tokens = quote! { vec![#(#tag_strs),*] };

    let kind_tokens = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => build_record_kind(&named.named, serde_rename_all.as_deref())?,
            Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                // Single-field tuple struct (`struct UserId(String);`).
                // Emit a transparent newtype kind so the Elm side gets
                // a `type alias` over the inner type and a delegating
                // codec. Multi-field tuple structs (`struct Pair(A, B)`)
                // have no obvious JSON shape and are still rejected
                // below.
                let only = unnamed.unnamed.first().expect("len == 1 was checked");
                let (inner_tokens, _) = rust_type_to_elm_repr(&only.ty)?;
                quote! {
                    elm_codegen_core::ElmTypeKind::Newtype {
                        inner: #inner_tokens,
                    }
                }
            }
            Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    name,
                    "ElmType only supports single-field tuple structs (newtypes); multi-field tuple structs have no canonical JSON shape — use a struct with named fields instead",
                ));
            }
            Fields::Unit => {
                return Err(syn::Error::new_spanned(
                    name,
                    "ElmType cannot be derived for unit structs",
                ));
            }
        },
        Data::Enum(data) => {
            let untagged = has_serde_untagged(&input.attrs);
            let tag_key = parse_serde_tag(&input.attrs);
            if untagged && tag_key.is_some() {
                return Err(syn::Error::new_spanned(
                    name,
                    "an enum can't be both #[serde(tag = \"...\")] and #[serde(untagged)]",
                ));
            }
            let representation = if untagged {
                EnumRepr::Untagged
            } else if let Some(key) = tag_key.as_deref() {
                EnumRepr::InternallyTagged(key)
            } else {
                EnumRepr::BareString
            };
            build_enum_kind(
                name,
                &data.variants,
                serde_rename_all.as_deref(),
                representation,
            )?
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "ElmType cannot be derived for unions",
            ));
        }
    };

    let rust_name_str = name.to_string();
    let expanded = quote! {
        impl elm_codegen_core::ElmType for #name {
            fn elm_type_info() -> elm_codegen_core::ElmTypeInfo {
                elm_codegen_core::ElmTypeInfo {
                    rust_name: #rust_name_str,
                    module_path: #module_tokens,
                    type_name: #type_name_str,
                    tags: #tags_tokens,
                    kind: #kind_tokens,
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

fn build_record_kind(
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
    serde_rename_all: Option<&str>,
) -> syn::Result<proc_macro2::TokenStream> {
    let field_tokens = build_field_info_list(fields, serde_rename_all)?;
    Ok(quote! {
        elm_codegen_core::ElmTypeKind::Record {
            fields: vec![#(#field_tokens),*],
        }
    })
}

/// Build the per-field `ElmFieldInfo { ... }` token streams for a set
/// of named fields. Shared between record structs and struct enum
/// variants so both honor the same `#[elm(...)]` and `#[serde(...)]`
/// attributes consistently.
fn build_field_info_list(
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
    serde_rename_all: Option<&str>,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
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
        } else if let Some(rename_all) = serde_rename_all {
            apply_rename_all(&rust_name_str, rename_all)
        } else {
            rust_name_str.clone()
        };

        let elm_name = if let Some(ref custom_name) = field_attrs.name {
            custom_name.clone()
        } else {
            snake_to_camel(&rust_name_str)
        };

        let (type_repr_tokens, is_optional) =
            if let Some(ref custom_type) = field_attrs.type_override {
                parse_type_override(custom_type)
            } else {
                rust_type_to_elm_repr(&field.ty)?
            };

        let decoder_tokens = match &field_attrs.decoder {
            Some(d) => quote! { Some(#d) },
            None => quote! { None },
        };
        let encoder_tokens = match &field_attrs.encoder {
            Some(e) => quote! { Some(#e) },
            None => quote! { None },
        };

        field_tokens.push(quote! {
            elm_codegen_core::ElmFieldInfo {
                rust_name: #json_key,
                elm_name: #elm_name,
                elm_type: #type_repr_tokens,
                is_optional: #is_optional,
                custom_decoder: #decoder_tokens,
                custom_encoder: #encoder_tokens,
            }
        });
    }

    Ok(field_tokens)
}

/// Local representation of the enum's serde-on-the-wire shape, used
/// to drive the variant payload rules below.
enum EnumRepr<'a> {
    BareString,
    InternallyTagged(&'a str),
    Untagged,
}

fn build_enum_kind(
    type_ident: &Ident,
    variants: &syn::punctuated::Punctuated<syn::Variant, Token![,]>,
    serde_rename_all: Option<&str>,
    repr: EnumRepr<'_>,
) -> syn::Result<proc_macro2::TokenStream> {
    if variants.is_empty() {
        return Err(syn::Error::new_spanned(
            type_ident,
            "ElmType requires at least one variant",
        ));
    }

    let has_struct_variant = variants
        .iter()
        .any(|v| matches!(v.fields, Fields::Named(_)));
    let has_payload_variant = variants.iter().any(|v| !matches!(v.fields, Fields::Unit));

    match &repr {
        EnumRepr::BareString => {
            if has_payload_variant {
                // serde's default for an enum without `tag` or
                // `untagged` is *externally* tagged: a non-unit
                // variant `Foo(T)` serializes as `{"Foo": <T>}` and
                // a struct variant as `{"Foo": {...}}`. That shape
                // is decodable in Elm but the codegen doesn't emit
                // it today; spell out both supported alternatives so
                // users don't have to guess.
                return Err(syn::Error::new_spanned(
                    type_ident,
                    "enums with non-unit variants need either #[serde(tag = \"...\")] (internally tagged: variants serialize as `{ \"<tag>\": \"<variant>\", ... }`) or #[serde(untagged)] (no discriminator; decoder tries variants in order). serde's default externally-tagged shape `{ \"<variant>\": <payload> }` isn't currently supported by ElmType",
                ));
            }
        }
        EnumRepr::InternallyTagged(_) => {
            // Internally-tagged enums serialize the tag as a field on the
            // outer object. Unit variants and struct variants are the
            // common cases. Single-field newtype variants are allowed
            // when the inner type serializes to a JSON object (struct or
            // another internally-tagged enum) — serde flattens its
            // fields next to the tag. Multi-field tuple variants are
            // still rejected because there's no object to flatten into.
            for v in variants {
                if let Fields::Unnamed(unnamed) = &v.fields {
                    if unnamed.unnamed.len() != 1 {
                        return Err(syn::Error::new_spanned(
                            v,
                            "internally-tagged enums (#[serde(tag = \"...\")]) only support newtype (single-field) tuple variants; wrap multiple values in a struct variant",
                        ));
                    }
                }
            }
            let _ = has_struct_variant;
        }
        EnumRepr::Untagged => {
            // Untagged accepts unit, single-field tuple, and struct
            // variants. n-ary tuple variants are still rejected.
            for v in variants {
                if let Fields::Unnamed(unnamed) = &v.fields {
                    if unnamed.unnamed.len() != 1 {
                        return Err(syn::Error::new_spanned(
                            v,
                            "untagged enums only support tuple variants with a single field; wrap multiple values in a struct variant",
                        ));
                    }
                }
            }
        }
    }

    let mut variant_tokens = Vec::new();
    for variant in variants {
        let rust_name_str = variant.ident.to_string();

        let variant_attrs = parse_variant_attrs(&variant.attrs)?;
        let elm_name = variant_attrs.name.unwrap_or_else(|| rust_name_str.clone());

        let json_tag = if let Some(rename) = parse_serde_variant_rename(&variant.attrs) {
            rename
        } else if let Some(rename_all) = serde_rename_all {
            apply_rename_all(&rust_name_str, rename_all)
        } else {
            rust_name_str.clone()
        };

        let payload_tokens = match &variant.fields {
            Fields::Unit => quote! { elm_codegen_core::ElmVariantPayload::Unit },
            Fields::Named(named) => {
                // Variant payload fields use the same per-field
                // attribute parsing as record fields. Default JSON
                // key follows the enclosing serde rename_all just
                // like record fields would.
                let fields_tokens = build_field_info_list(&named.named, serde_rename_all)?;
                quote! {
                    elm_codegen_core::ElmVariantPayload::Struct(
                        vec![#(#fields_tokens),*]
                    )
                }
            }
            Fields::Unnamed(unnamed) => {
                // Single-field newtype variant under EnumRepr::Untagged
                // (inner type serialized directly) or
                // EnumRepr::InternallyTagged (inner fields flattened next
                // to the tag). Multi-field tuple variants are rejected
                // earlier for both representations.
                let only = unnamed
                    .unnamed
                    .first()
                    .expect("newtype variant must have one field");
                let (inner_tokens, _is_optional) = rust_type_to_elm_repr(&only.ty)?;
                quote! { elm_codegen_core::ElmVariantPayload::Newtype(#inner_tokens) }
            }
        };

        variant_tokens.push(quote! {
            elm_codegen_core::ElmVariantInfo {
                rust_name: #rust_name_str,
                elm_name: #elm_name,
                json_tag: #json_tag,
                payload: #payload_tokens,
            }
        });
    }

    let representation_tokens = match repr {
        EnumRepr::BareString => quote! { elm_codegen_core::EnumRepresentation::BareString },
        EnumRepr::InternallyTagged(key) => quote! {
            elm_codegen_core::EnumRepresentation::InternallyTagged { tag_key: #key }
        },
        EnumRepr::Untagged => quote! { elm_codegen_core::EnumRepresentation::Untagged },
    };

    Ok(quote! {
        elm_codegen_core::ElmTypeKind::Enum {
            variants: vec![#(#variant_tokens),*],
            representation: #representation_tokens,
        }
    })
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
    encoder: Option<String>,
}

fn parse_field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut result = FieldAttrs {
        skip: false,
        type_override: None,
        name: None,
        decoder: None,
        encoder: None,
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
            } else if meta.path.is_ident("encoder") {
                result.encoder = Some(parse_str_value(&meta)?);
                Ok(())
            } else {
                Err(meta.error("unknown elm field attribute"))
            }
        })?;
    }

    Ok(result)
}

struct VariantAttrs {
    name: Option<String>,
}

fn parse_variant_attrs(attrs: &[Attribute]) -> syn::Result<VariantAttrs> {
    let mut result = VariantAttrs { name: None };
    for attr in attrs {
        if !attr.path().is_ident("elm") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                result.name = Some(parse_str_value(&meta)?);
                Ok(())
            } else {
                Err(meta.error("unknown elm variant attribute"))
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

fn parse_serde_tag(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut tag = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("tag") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    tag = Some(s.value());
                }
            }
            Ok(())
        });
        if tag.is_some() {
            return tag;
        }
    }
    None
}

/// Detect `#[serde(untagged)]`. Untagged enums let serde try each
/// variant in order and pick the first that matches; there's no
/// discriminator on the wire. We don't generate that shape, so we
/// reject it at derive time rather than emit a broken codec.
fn has_serde_untagged(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("untagged") {
                found = true;
            }
            Ok(())
        });
        if found {
            return true;
        }
    }
    false
}

fn parse_serde_field_rename(attrs: &[Attribute]) -> Option<String> {
    parse_serde_rename(attrs)
}

fn parse_serde_variant_rename(attrs: &[Attribute]) -> Option<String> {
    parse_serde_rename(attrs)
}

fn parse_serde_rename(attrs: &[Attribute]) -> Option<String> {
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

/// Apply a serde `rename_all` convention. Accepts either a snake_case
/// (struct field) or a PascalCase (enum variant) input identifier and
/// produces the renamed form. Variant idents are first split into
/// snake_case so the same conversion table works for both.
fn apply_rename_all(name: &str, convention: &str) -> String {
    let snake = if name.contains('_') || name.chars().all(|c| !c.is_uppercase()) {
        name.to_string()
    } else {
        pascal_to_snake(name)
    };
    match convention {
        "lowercase" => snake.replace('_', "").to_lowercase(),
        "UPPERCASE" => snake.replace('_', "").to_uppercase(),
        "PascalCase" => snake_to_pascal(&snake),
        "camelCase" => snake_to_camel(&snake),
        "snake_case" => snake,
        "SCREAMING_SNAKE_CASE" => snake.to_uppercase(),
        "kebab-case" => snake.replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => snake.replace('_', "-").to_uppercase(),
        _ => name.to_string(),
    }
}

fn pascal_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i != 0 {
            out.push('_');
        }
        for lc in c.to_lowercase() {
            out.push(lc);
        }
    }
    out
}

fn snake_to_pascal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = true;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

// ── Type mapping ────────────────────────────────────────────────────

/// Parse the string passed to `#[elm(type = "...")]`. Recognizes the
/// primitive Elm names so an override of `"String"` produces real
/// `ElmTypeRepr::String` (not `Custom("String")` which would generate
/// nonsense like `encodeString`/`stringDecoder`).
///
/// `Maybe T` and `List T` wrappers are parsed recursively. Any unknown
/// shape falls through to `Custom(...)`. Returns the optionality flag
/// (`true` for `Maybe ...`, matching the auto-detection behavior).
fn parse_type_override(s: &str) -> (proc_macro2::TokenStream, bool) {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("Maybe ") {
        let (inner, _) = parse_type_override(rest);
        return (
            quote! { elm_codegen_core::ElmTypeRepr::Maybe(Box::new(#inner)) },
            true,
        );
    }
    if let Some(rest) = trimmed.strip_prefix("List ") {
        let (inner, _) = parse_type_override(rest);
        return (
            quote! { elm_codegen_core::ElmTypeRepr::List(Box::new(#inner)) },
            false,
        );
    }
    if let Some(rest) = trimmed.strip_prefix("Dict String ") {
        let (inner, _) = parse_type_override(rest);
        return (
            quote! { elm_codegen_core::ElmTypeRepr::Dict(Box::new(#inner)) },
            false,
        );
    }
    let primitive = match trimmed {
        "String" => Some(quote! { elm_codegen_core::ElmTypeRepr::String }),
        "Int" => Some(quote! { elm_codegen_core::ElmTypeRepr::Int }),
        "Float" => Some(quote! { elm_codegen_core::ElmTypeRepr::Float }),
        "Bool" => Some(quote! { elm_codegen_core::ElmTypeRepr::Bool }),
        "Time.Posix" => Some(quote! { elm_codegen_core::ElmTypeRepr::Posix }),
        "Encode.Value" | "Decode.Value" | "Json.Encode.Value" | "Json.Decode.Value" => {
            Some(quote! { elm_codegen_core::ElmTypeRepr::Value })
        }
        _ => None,
    };
    if let Some(tokens) = primitive {
        return (tokens, false);
    }
    (
        quote! { elm_codegen_core::ElmTypeRepr::Custom(#trimmed.to_string()) },
        false,
    )
}

fn rust_type_to_elm_repr(ty: &Type) -> syn::Result<(proc_macro2::TokenStream, bool)> {
    match ty {
        Type::Path(type_path) => path_to_elm_repr(type_path),
        // `(A, B)` / `(A, B, C)`. Elm only has 2- and 3-tuples, and a
        // single-element parenthesised type in Rust isn't a tuple at
        // all (it's just the type), so we explicitly enforce the
        // 2..=3 range.
        Type::Tuple(t) => {
            let arity = t.elems.len();
            if !(2..=3).contains(&arity) {
                return Err(syn::Error::new_spanned(
                    t,
                    "ElmType only supports 2- and 3-tuples; for higher arity wrap the values in a struct with named fields",
                ));
            }
            let mut element_tokens = Vec::with_capacity(arity);
            for elem in &t.elems {
                let (inner_tokens, _) = rust_type_to_elm_repr(elem)?;
                element_tokens.push(inner_tokens);
            }
            Ok((
                quote! {
                    elm_codegen_core::ElmTypeRepr::Tuple(vec![#(#element_tokens),*])
                },
                false,
            ))
        }
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
        // `chrono::NaiveDateTime` and `NaiveTime` serialize as bare
        // ISO-style strings without timezone info. We hand them to
        // Elm as `String` rather than `Time.Posix` because Posix
        // implies UTC; users who want parsed values should layer their
        // own decoder via `#[elm(decoder = "...")]`.
        "NaiveDateTime" | "NaiveTime" => {
            Ok((quote! { elm_codegen_core::ElmTypeRepr::String }, false))
        }
        // `uuid::Uuid` serializes as a hyphenated hex string by default,
        // so it's an Elm `String` on the wire. Recognized by name only.
        "Uuid" => Ok((quote! { elm_codegen_core::ElmTypeRepr::String }, false)),
        // `serde_json::Value` (or `Value` brought in via `use`). Treated
        // as a raw JSON blob: rendered as Elm `Encode.Value`, encoded
        // with identity, decoded with `Decode.value`. Recognized by name
        // only; users with a same-named domain type should use
        // `#[elm(type = "...")]` to override.
        "Value" => Ok((quote! { elm_codegen_core::ElmTypeRepr::Value }, false)),
        // `Patch<T>` and `PatchNullable<T>` are common JSON merge-patch
        // wrapper types: the field may be omitted from the wire payload
        // and (for the nullable variant) may also be `null`. Both map
        // to Elm `Maybe T` since on the wire they look like an optional
        // field. Recognized by name only; users with differently-named
        // wrappers should use `#[elm(type = "...")]` overrides for now.
        "Option" | "Patch" | "PatchNullable" => {
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
        // String-keyed maps. Renders as Elm `Dict String T`. JSON
        // object keys are always strings, so non-`String` key types
        // would silently mis-encode; we reject at derive time.
        "HashMap" | "BTreeMap" => {
            let key = extract_first_generic_arg(segment)?;
            if !type_is_string(&key) {
                return Err(syn::Error::new_spanned(
                    &key,
                    "ElmType only supports string-keyed maps (HashMap<String, T> / BTreeMap<String, T>); JSON objects use string keys, so non-String key types can't round-trip. Use #[elm(type = \"Encode.Value\")] on the field if you need an opaque blob.",
                ));
            }
            let value = extract_second_generic_arg(segment)?;
            let (value_tokens, _) = rust_type_to_elm_repr(&value)?;
            Ok((
                quote! { elm_codegen_core::ElmTypeRepr::Dict(Box::new(#value_tokens)) },
                false,
            ))
        }
        // Transparent wrappers: traverse into the inner type. Serde
        // encodes `Box<T>`/`Arc<T>`/`Rc<T>` exactly like `T`, so the
        // Elm side should mirror that. `Cow<'_, T>` follows the same
        // rule (with two generic arguments — the lifetime is dropped
        // by `extract_single_generic_arg` since it filters non-Type
        // args).
        "Box" | "Arc" | "Rc" | "Cow" => {
            let inner = extract_single_generic_arg(segment)?;
            rust_type_to_elm_repr(&inner)
        }
        other => Ok((
            quote! { elm_codegen_core::ElmTypeRepr::Custom(#other.to_string()) },
            false,
        )),
    }
}

/// True if the given type is a `String` path (bare `String` or
/// `std::string::String`). Used to validate map key types.
fn type_is_string(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            return seg.ident == "String";
        }
    }
    false
}

fn extract_single_generic_arg(segment: &syn::PathSegment) -> syn::Result<Type> {
    match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => {
            for arg in &args.args {
                if let syn::GenericArgument::Type(ty) = arg {
                    return Ok(ty.clone());
                }
            }
            Err(syn::Error::new_spanned(segment, "expected a type argument"))
        }
        _ => Err(syn::Error::new_spanned(
            segment,
            "expected angle-bracketed arguments",
        )),
    }
}

fn extract_first_generic_arg(segment: &syn::PathSegment) -> syn::Result<Type> {
    extract_single_generic_arg(segment)
}

fn extract_second_generic_arg(segment: &syn::PathSegment) -> syn::Result<Type> {
    match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => {
            let mut type_args = args.args.iter().filter_map(|a| {
                if let syn::GenericArgument::Type(ty) = a {
                    Some(ty)
                } else {
                    None
                }
            });
            let _ = type_args.next();
            type_args
                .next()
                .cloned()
                .ok_or_else(|| syn::Error::new_spanned(segment, "expected two type arguments"))
        }
        _ => Err(syn::Error::new_spanned(
            segment,
            "expected angle-bracketed arguments",
        )),
    }
}

// ── #[elm_endpoint(...)] attribute macro ────────────────────────────

/// Attribute macro that registers an HTTP handler with the
/// `elm-codegen-http` registry so the codegen can emit a matching
/// Elm request function.
///
/// # Syntax
///
/// ```ignore
/// #[elm_endpoint(
///     GET, "/api/v1/persons/{person_id}",
///     module = "Api.Generated.Person",
///     name   = "getPerson",            // optional; default = camelCase(fn name)
///     tags   = "read",                  // optional; comma-separated
/// )]
/// async fn get_person(...) -> Result<Json<Person>, HttpError> { ... }
/// ```
///
/// The macro doesn't inspect extractor types beyond textually
/// dispatching to `<T as ElmExtractor>::elm_extractor_info()` and
/// `<R as ElmResponse>::elm_response_info()`. All interpretation is
/// done by trait impls.
///
/// The original function is emitted unchanged.
#[proc_macro_attribute]
pub fn elm_endpoint(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as EndpointArgs);
    let item_fn = parse_macro_input!(input as ItemFn);
    match impl_elm_endpoint(args, item_fn) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct EndpointArgs {
    method: Ident,
    path: LitStr,
    module: Option<String>,
    name_override: Option<String>,
    tags: Option<String>,
}

impl Parse for EndpointArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let method: Ident = input.parse()?;
        input.parse::<Token![,]>()?;
        let path: LitStr = input.parse()?;

        let mut args = EndpointArgs {
            method,
            path,
            module: None,
            name_override: None,
            tags: None,
        };

        while input.parse::<Token![,]>().is_ok() {
            if input.is_empty() {
                break;
            }
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: LitStr = input.parse()?;
            let key_str = key.to_string();
            match key_str.as_str() {
                "module" => args.module = Some(value.value()),
                "name" => args.name_override = Some(value.value()),
                "tags" => args.tags = Some(value.value()),
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown elm_endpoint attribute: {key_str}"),
                    ))
                }
            }
        }

        Ok(args)
    }
}

fn impl_elm_endpoint(args: EndpointArgs, item_fn: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let method_variant = match args.method.to_string().to_uppercase().as_str() {
        "GET" => quote! { Get },
        "POST" => quote! { Post },
        "PUT" => quote! { Put },
        "PATCH" => quote! { Patch },
        "DELETE" => quote! { Delete },
        other => {
            return Err(syn::Error::new(
                args.method.span(),
                format!("unsupported HTTP method: {other}"),
            ))
        }
    };

    let module_str = args.module.ok_or_else(|| {
        syn::Error::new(
            args.method.span(),
            "missing required `module = \"Api.Foo.Bar\"` attribute",
        )
    })?;
    let module_segments: Vec<&str> = module_str.split('.').collect();

    let handler_name = item_fn.sig.ident.to_string();
    let elm_function_name = args
        .name_override
        .unwrap_or_else(|| snake_to_camel(&handler_name));

    let tag_strs: Vec<String> = args
        .tags
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let path_str = args.path.value();

    // Collect parameter types in source order, skipping receivers
    // (`self`/`&self`) which shouldn't appear on Axum handlers but
    // are tolerated defensively.
    let mut param_dispatches = Vec::new();
    for arg in &item_fn.sig.inputs {
        match arg {
            FnArg::Typed(pat_ty) => {
                let ty = &pat_ty.ty;
                param_dispatches.push(quote! {
                    params.push(<#ty as ::elm_codegen_http::ElmExtractor>::elm_extractor_info());
                });
            }
            FnArg::Receiver(_) => continue,
        }
    }

    let response_dispatch = match &item_fn.sig.output {
        ReturnType::Type(_, ty) => quote! {
            <#ty as ::elm_codegen_http::ElmResponse>::elm_response_info()
        },
        ReturnType::Default => {
            return Err(syn::Error::new(
                item_fn.sig.ident.span(),
                "elm_endpoint handlers must declare a return type",
            ))
        }
    };

    let builder_ident = format_ident!("__elm_endpoint_build_{}", handler_name);

    let registration = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #builder_ident() -> ::elm_codegen_http::ElmEndpointInfo {
            let mut params: Vec<::elm_codegen_http::ExtractorInfo> = Vec::new();
            #(#param_dispatches)*

            ::elm_codegen_http::ElmEndpointInfo {
                handler_name: #handler_name,
                elm_function_name: #elm_function_name,
                elm_module_path: &[#(#module_segments),*],
                method: ::elm_codegen_http::HttpMethod::#method_variant,
                path_template: #path_str,
                params,
                response: #response_dispatch,
                tags: &[#(#tag_strs),*],
            }
        }

        ::elm_codegen_http::inventory::submit! {
            ::elm_codegen_http::ElmEndpointRegistration {
                build: #builder_ident,
            }
        }
    };

    Ok(quote! {
        #item_fn
        #registration
    })
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
