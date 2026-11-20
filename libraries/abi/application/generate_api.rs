//! Build-time SDK generation from the unexpanded Rust ABI.
use std::{collections::BTreeSet, fmt::Write, fs, path::Path};
use syn::{Fields, Item, Type};

// These slots remain in the table when the runtime omits networking.
const NETWORK_SLOTS: &[&str] = &[
    "network_close",
    "tcp_connect",
    "tcp_listen",
    "tcp_accept",
    "tcp_close",
    "tcp_listener_close",
    "tcp_shutdown",
    "tcp_send",
    "tcp_receive",
    "tcp_local_address",
    "tcp_peer_address",
    "udp_bind",
    "udp_close",
    "udp_send_to",
    "udp_receive_from",
    "network_resolve",
];

pub fn generate(root: &Path, out: &Path, export: &cbindgen::ExportConfig) {
    let source = fs::read_to_string(root.join("src/api.rs")).unwrap();
    let syntax = syn::parse_file(&source).unwrap();
    let table = syntax
        .items
        .iter()
        .find_map(|item| match item {
            Item::Struct(item) if item.ident == "XilaApi" => Some(item),
            _ => None,
        })
        .expect("XilaApi struct");
    let Fields::Named(fields) = &table.fields else {
        panic!("XilaApi must have named fields");
    };

    let generated = "/* Generated from XilaApi. Do not edit. */\n";
    let mut header = format!(
        "{generated}\
        #ifndef XILA_SDK_STUBS_H\n#define XILA_SDK_STUBS_H\n\n\
        #include \"application.h\"\n\n\
        #ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n\
        int32_t _xila_start(const XilaStartup *startup);\n\n"
    );
    let mut stubs = format!(
        "{generated}\
        #include \"stubs.h\"\n\n\
        extern const XilaApi *xila_sdk_api;\n"
    );
    let mut fallback = String::from(
        "\nconst XilaApi xila_sdk_fallback_api = {\n    \
         .version = (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR,\n    \
         .size = sizeof(XilaApi),\n",
    );
    let mut network_slots: BTreeSet<_> = NETWORK_SLOTS.iter().copied().collect();
    let mut slots = String::from("# Zero-based slot\tC symbol\n");
    let mut disabled_network = String::new();
    let mut layout = String::from(
        "const _: () = {\n\
         let mut offset = 8;\n\
         let pointer_size = core::mem::size_of::<unsafe extern \"C\" fn() -> Status>();\n\
         assert!(core::mem::offset_of!(XilaApi, version) == 0);\n\
         assert!(core::mem::offset_of!(XilaApi, size) == 4);\n",
    );

    let mut fields = fields.named.iter();
    for name in ["version", "size"] {
        let field = fields.next().expect("XilaApi header");
        assert_eq!(field.ident.as_ref().unwrap(), name);
        assert!(matches!(&field.ty, Type::Path(ty) if ty.path.is_ident("u32")));
    }
    for (index, field) in fields.enumerate() {
        let name = field.ident.as_ref().unwrap().to_string();
        let Type::FnPtr(signature) = &field.ty else {
            panic!("XilaApi.{name} must be a function pointer");
        };
        assert!(signature.unsafety.is_some());
        assert_eq!(
            signature
                .abi
                .as_ref()
                .unwrap()
                .name
                .as_ref()
                .unwrap()
                .value(),
            "C"
        );
        assert!(signature.variadic.is_none());
        assert!(matches!(
            &signature.output,
            syn::ReturnType::Type(_, ty) if matches!(&**ty, Type::Path(ty) if ty.path.is_ident("Status"))
        ));

        let mut arguments = Vec::new();
        let mut parameters = Vec::new();
        for parameter in &signature.inputs {
            let argument = parameter
                .name
                .as_ref()
                .expect("slot parameter name")
                .0
                .to_string();
            parameters.push(format!("{} {argument}", c_type(&parameter.ty, export)));
            arguments.push(argument);
        }
        let parameters = if parameters.is_empty() {
            "void".into()
        } else {
            parameters.join(", ")
        };
        // Generate a correctly typed fallback for each slot. Calling through a
        // cast to a single function with a different signature would be UB.
        writeln!(
            stubs,
            "\nstatic XilaStatus unavailable_{name}({parameters}) {{"
        )
        .unwrap();
        for argument in &arguments {
            writeln!(stubs, "    (void){argument};").unwrap();
        }
        writeln!(stubs, "    return XILA_STATUS_UNAVAILABLE;\n}}").unwrap();
        writeln!(fallback, "    .{name} = unavailable_{name},").unwrap();

        let arguments = arguments.join(", ");
        let declaration = format!("XilaStatus xila_{name}({parameters})");
        writeln!(header, "{declaration};").unwrap();
        writeln!(
            stubs,
            "\n{declaration} {{\n    return xila_sdk_api->{name}({arguments});\n}}"
        )
        .unwrap();
        writeln!(slots, "{index}\txila_{name}").unwrap();
        writeln!(
            layout,
            "assert!(core::mem::offset_of!(XilaApi, {name}) == offset);\noffset += pointer_size;"
        )
        .unwrap();

        if network_slots.remove(name.as_str()) {
            let mut parameters = Vec::new();
            for input in &signature.inputs {
                let ty = &input.ty;
                let tokens = quote::quote!(#ty);
                parameters.push(format!("_: {tokens}"));
            }
            let parameters = parameters.join(", ");
            writeln!(
                disabled_network,
                "unsafe extern \"C\" fn xila_{name}({parameters}) -> Status {{ Status::Unsupported }}"
            ).unwrap();
        }
    }
    assert!(
        network_slots.is_empty(),
        "Unknown network slots: {network_slots:?}"
    );
    fallback.push_str("};\n");
    stubs.push_str(&fallback);
    header.push_str("\n#ifdef __cplusplus\n}\n#endif\n\n#endif\n");
    writeln!(
        layout,
        "assert!(core::mem::size_of::<XilaApi>() == offset);\n}};"
    )
    .unwrap();
    for (name, contents) in [
        ("stubs.h", header),
        ("stubs.c", stubs),
        ("slots.tsv", slots),
        ("disabled_network.rs", disabled_network),
        ("api_layout.rs", layout),
    ] {
        fs::write(out.join(name), contents).unwrap();
    }
}

fn c_type(ty: &Type, export: &cbindgen::ExportConfig) -> String {
    match ty {
        Type::Ptr(ty) => {
            let pointee = c_type(&ty.elem, export);
            if matches!(ty.mutability, syn::PointerMutability::Const(_)) {
                format!("{pointee} const *")
            } else {
                format!("{pointee} *")
            }
        }
        Type::Path(ty) => {
            let name = ty.path.segments.last().unwrap().ident.to_string();
            if let Some(name) = export.rename.get(&name) {
                return name.clone();
            }
            match name.as_str() {
                "c_char" => "char",
                "c_void" => "void",
                "usize" => "size_t",
                "i32" => "int32_t",
                "u32" => "uint32_t",
                "u64" => "uint64_t",
                "i64" => "int64_t",
                _ => panic!("Unsupported SDK slot type: {name}"),
            }
            .into()
        }
        _ => panic!("Unsupported SDK slot type"),
    }
}
