//! Generate the dependency-free guest ABI and target-side layout comparisons.

use std::{collections::BTreeSet, fmt::Write, fs, path::Path};

use quote::quote;
use syn::{
    Attribute, Fields, Item, ItemEnum, ItemStruct, Meta, Token, Visibility, parse::Parser,
    punctuated::Punctuated,
};

fn read(path: &Path) -> syn::File {
    syn::parse_file(&fs::read_to_string(path).unwrap()).unwrap()
}

pub fn generate(root: &Path, out: &Path, export: &cbindgen::ExportConfig, table: &ItemStruct) {
    let src = root.join("src");
    let lib = read(&src.join("lib.rs"));
    // Reuse the C export configuration to select the public wire types.
    let mut remaining: BTreeSet<_> = export
        .rename
        .keys()
        .cloned()
        .chain(export.include.iter().cloned())
        .collect();
    let mut bindings = String::from("use core::ffi::{c_char, c_void};\n");
    let mut layout = String::from("use crate::guest_bindings as guest;\n");
    let status = read(&out.join("status.rs"));
    let version = read(&out.join("version.rs"));

    let mut items = vec![(None, Item::Struct(table.clone()))];
    for item in &lib.items {
        if let Item::Mod(module) = item {
            let name = &module.ident;
            if module.content.is_none() && name != "api" {
                let file = src.join(format!("{name}.rs"));
                let file = if file.is_file() {
                    file
                } else {
                    src.join(format!("{name}/mod.rs"))
                };
                items.extend(
                    read(&file)
                        .items
                        .into_iter()
                        .map(|item| (Some(module), item)),
                );
            }
        }
    }
    items.extend(lib.items.iter().cloned().map(|item| (None, item)));
    items.extend(status.items.into_iter().map(|item| (None, item)));
    items.extend(version.items.into_iter().map(|item| (None, item)));

    for (module, item) in items {
        let check_export = |attrs: &[Attribute], name: &syn::Ident| {
            reject_feature_cfg(attrs, name);
            if let Some(module) = module {
                reject_feature_cfg(&module.attrs, &module.ident);
            }
        };
        match item {
            Item::Struct(mut record) if remaining.remove(&record.ident.to_string()) => {
                check_export(&record.attrs, &record.ident);
                let name = &record.ident;
                if record.attrs.iter().any(|attr| attr.path().is_ident("repr")) {
                    let Fields::Named(fields) = &mut record.fields else {
                        panic!("Wire record {name} must have named fields");
                    };
                    check_type(&mut layout, name, &[]);
                    for field in &mut fields.named {
                        let member = field.ident.as_ref().unwrap();
                        reject_feature_cfg(&field.attrs, member);
                        writeln!(
                            layout,
                            "const _: () = assert!(core::mem::offset_of!(crate::{name}, {member}) \
                             == core::mem::offset_of!(guest::{name}, {member}));"
                        )
                        .unwrap();
                        // Let the host layout checks see private guest fields while keeping
                        // them private to xila-sys users.
                        if matches!(field.vis, Visibility::Inherited) {
                            field.vis = syn::parse_quote!(pub(crate));
                        }
                    }
                    writeln!(bindings, "{}", quote!(#record)).unwrap();
                    let zero = match name.to_string().as_str() {
                        "Mutex" => Some(quote!(Self {
                            owner: 0,
                            depth: 0,
                            recursive: false,
                            waiters: core::ptr::null_mut(),
                        })),
                        "Condition" => Some(quote!(Self {
                            waiters: core::ptr::null_mut()
                        })),
                        _ => None,
                    };
                    if let Some(zero) = zero {
                        writeln!(
                            bindings,
                            "{}",
                            quote! {
                                impl #name {
                                    /// Initial storage, before any host operation.
                                    pub const ZERO: Self = #zero;
                                }
                            }
                        )
                        .unwrap();
                    }
                } else {
                    // Only pointer layout is public for opaque handles. Their host
                    // pointees can have different layouts under different features.
                    let docs = record
                        .attrs
                        .iter()
                        .filter(|attr| attr.path().is_ident("doc"));
                    writeln!(
                        bindings,
                        "{}",
                        quote! {
                            #(#docs)*
                            #[repr(C)]
                            pub struct #name {
                                _opaque: [u8; 0],
                                _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
                            }
                        }
                    )
                    .unwrap();
                    writeln!(
                        layout,
                        "const _: () = assert!(core::mem::size_of::<*const crate::{name}>() \
                         == core::mem::size_of::<*const guest::{name}>() \
                         && core::mem::align_of::<*const crate::{name}>() \
                         == core::mem::align_of::<*const guest::{name}>());"
                    )
                    .unwrap();
                }
            }
            Item::Type(alias) if remaining.remove(&alias.ident.to_string()) => {
                check_export(&alias.attrs, &alias.ident);
                check_type(&mut layout, &alias.ident, &alias.attrs);
                writeln!(bindings, "{}", quote!(#alias)).unwrap();
            }
            Item::Enum(enumeration) if remaining.remove(&enumeration.ident.to_string()) => {
                check_export(&enumeration.attrs, &enumeration.ident);
                generate_enum(&mut bindings, &mut layout, &enumeration);
            }
            Item::Const(constant) if is_exported_constant(&constant) => {
                check_export(&constant.attrs, &constant.ident);
                let name = &constant.ident;
                let cfg = constant
                    .attrs
                    .iter()
                    .filter(|attr| attr.path().is_ident("cfg"));
                writeln!(
                    layout,
                    "{} const _: () = assert!(crate::{name} == guest::{name});",
                    quote!(#(#cfg)*),
                )
                .unwrap();
                writeln!(bindings, "{}", quote!(#constant)).unwrap();
            }
            _ => {}
        }
    }
    assert!(remaining.is_empty(), "Missing wire types: {remaining:?}");

    let bindings = prettyplease::unparse(&syn::parse_file(&bindings).unwrap());
    fs::write(
        out.join("bindings.rs"),
        format!("// Generated from the native application ABI. Do not edit.\n\n{bindings}"),
    )
    .unwrap();
    fs::write(out.join("rust_layout.rs"), layout).unwrap();
}

fn is_exported_constant(item: &syn::ItemConst) -> bool {
    matches!(item.vis, Visibility::Public(_))
        && item.ident.to_string().starts_with("XILA_")
        && item.ident != "XILA_API"
}

fn reject_feature_cfg(attrs: &[Attribute], name: &syn::Ident) {
    fn mentions_feature(meta: &Meta) -> bool {
        match meta {
            Meta::NameValue(value) => value.path.is_ident("feature"),
            Meta::List(list)
                if ["cfg", "cfg_attr", "all", "any", "not"]
                    .iter()
                    .any(|name| list.path.is_ident(name)) =>
            {
                Punctuated::<Meta, Token![,]>::parse_terminated
                    .parse2(list.tokens.clone())
                    .unwrap()
                    .iter()
                    .any(mentions_feature)
            }
            _ => false,
        }
    }

    assert!(
        !attrs.iter().any(|attr| mentions_feature(&attr.meta)),
        "Exported native ABI item {name} cannot depend on a Cargo feature"
    );
}

fn generate_enum(bindings: &mut String, layout: &mut String, enumeration: &ItemEnum) {
    let name = &enumeration.ident;
    let attrs = &enumeration.attrs;
    let repr = attrs.iter().find(|attr| attr.path().is_ident("repr"));
    assert!(
        repr.and_then(|attr| attr.parse_args::<syn::Ident>().ok())
            .is_some_and(|repr| repr == "u32"),
        "Wire enum {name} must have repr(u32)"
    );
    check_type(layout, name, attrs);

    let attrs = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc") || attr.path().is_ident("cfg"));
    // The host can return future discriminants. A Rust enum would make those invalid values.
    let mut constants = Vec::new();
    for variant in &enumeration.variants {
        let member = &variant.ident;
        reject_feature_cfg(&variant.attrs, member);
        assert!(
            matches!(&variant.fields, Fields::Unit),
            "Wire enum {name} must have unit variants"
        );
        let (_, value) = variant
            .discriminant
            .as_ref()
            .expect("Wire enum discriminant");
        let attrs = &variant.attrs;
        constants.push(quote!(#(#attrs)* pub const #member: Self = Self(#value);));
        let cfg = enumeration
            .attrs
            .iter()
            .chain(attrs)
            .filter(|attr| attr.path().is_ident("cfg"));
        writeln!(
            layout,
            "{} const _: () = assert!(crate::{name}::{member} as u32 == guest::{name}::{member}.0);",
            quote!(#(#cfg)*),
        )
        .unwrap();
    }
    let cfg = enumeration
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg"));
    let visibility = &enumeration.vis;
    writeln!(
        bindings,
        "{}",
        quote! {
            #(#attrs)*
            #[repr(transparent)]
            #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
            #visibility struct #name(pub u32);

            #(#cfg)*
            #[allow(non_upper_case_globals)]
            impl #name {
                #(#constants)*
            }
        }
    )
    .unwrap();
}

fn check_type(layout: &mut String, name: &syn::Ident, attrs: &[syn::Attribute]) {
    let cfg = attrs.iter().filter(|attr| attr.path().is_ident("cfg"));
    writeln!(
        layout,
        "{} const _: () = assert!(core::mem::size_of::<crate::{name}>() \
         == core::mem::size_of::<guest::{name}>() \
         && core::mem::align_of::<crate::{name}>() == core::mem::align_of::<guest::{name}>());",
        quote!(#(#cfg)*),
    )
    .unwrap();
}
