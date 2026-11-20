//! Generate the dependency-free guest ABI and target-side layout comparisons.

use std::{collections::BTreeSet, fmt::Write, fs, path::Path};

use quote::quote;
use syn::{Fields, Item, ItemStruct, Visibility};

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

    let mut items = vec![Item::Struct(table.clone())];
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
                items.extend(read(&file).items);
            }
        }
    }
    items.extend(lib.items);
    items.extend(status.items);

    for item in items {
        match item {
            Item::Struct(mut record) if remaining.remove(&record.ident.to_string()) => {
                let name = &record.ident;
                if record.attrs.iter().any(|attr| attr.path().is_ident("repr")) {
                    let Fields::Named(fields) = &mut record.fields else {
                        panic!("Wire record {name} must have named fields");
                    };
                    check_type(&mut layout, name, &[]);
                    for field in &mut fields.named {
                        let member = field.ident.as_ref().unwrap();
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
                check_type(&mut layout, &alias.ident, &alias.attrs);
                writeln!(bindings, "{}", quote!(#alias)).unwrap();
            }
            Item::Enum(status) if remaining.remove(&status.ident.to_string()) => {
                check_type(&mut layout, &status.ident, &status.attrs);
                for variant in &status.variants {
                    let name = &variant.ident;
                    writeln!(
                        layout,
                        "const _: () = assert!(crate::Status::{name} as u32 == guest::Status::{name} as u32);"
                    )
                    .unwrap();
                }
                writeln!(bindings, "{}", quote!(#status)).unwrap();
            }
            Item::Const(constant)
                if matches!(constant.vis, Visibility::Public(_))
                    && constant.ident.to_string().starts_with("XILA_")
                    && constant.ident != "XILA_API" =>
            {
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

    let bindings = prettyplease::unparse(&syn2::parse_file(&bindings).unwrap());
    fs::write(
        out.join("bindings.rs"),
        format!("// Generated from the native application ABI. Do not edit.\n\n{bindings}"),
    )
    .unwrap();
    fs::write(out.join("rust_layout.rs"), layout).unwrap();
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
