//! The native SDK's graphics bindings are generated from the same declaration
//! as the Wasm guest API and committed next to the ABI they call through.
//! This test regenerates them and fails when a committed file is stale;
//! `XILA_UPDATE_HEADER=1` rewrites them.

use graphics_bindgen::{EVENT_CODE_SOURCE, abi, enumeration, guest, guest_constants};
use std::{fs, path::PathBuf};

fn abi_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../libraries/abi/application")
}

fn format(source: &str) -> String {
    prettyplease::unparse(&syn::parse_file(source).expect("generated Rust parses"))
}

/// Rust, as the single module `xila-sys` compiles.
fn rust_module(calls: &[guest::GuestCall], enumeration: &str, constants: &str) -> String {
    let functions = guest::rust_functions(calls);
    let source = format!(
        "use super::call;\nuse core::ffi::c_void;\n{}\n{EVENT_CODE_SOURCE}\n{enumeration}\n{constants}\n{functions}",
        guest::TYPES_SOURCE
    );
    format!(
        "// Generated from the graphics ABI declaration. Do not edit.\n\n{}",
        format(&source)
    )
}

fn header(calls: &[guest::GuestCall], enumeration: &str, constants: &str) -> String {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-sdk");
    fs::create_dir_all(&directory).unwrap();
    let write = |name: &str, source: String| {
        let path = directory.join(name);
        fs::write(&path, source).unwrap();
        path
    };
    let declarations = write(
        "declarations.rs",
        format(&format!(
            "{}\n{}",
            guest::TYPES_SOURCE,
            guest::c_declarations(calls)
        )),
    );
    let calls_and_constants = write("calls.rs", format(&format!("{enumeration}\n{constants}")));
    let events = write("event_code.rs", EVENT_CODE_SOURCE.to_string());
    let configuration = cbindgen::Config {
        language: cbindgen::Language::C,
        cpp_compat: true,
        include_guard: Some("XILA_GRAPHICS_H".into()),
        sys_includes: vec!["stddef.h".into(), "stdbool.h".into(), "stdint.h".into()],
        usize_is_size_t: true,
        no_includes: true,
        export: cbindgen::ExportConfig {
            prefix: Some(guest::C_PREFIX.into()),
            // Only the generated definitions refer to the call IDs, and the event
            // constant is a literal, so nothing else refers to the event codes.
            include: vec!["FunctionCall".into(), "EventCode".into()],
            ..Default::default()
        },
        enumeration: cbindgen::EnumConfig {
            rename_variants: cbindgen::RenameRule::QualifiedScreamingSnakeCase,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut output = Vec::new();
    cbindgen::Builder::new()
        .with_src(declarations)
        .with_src(calls_and_constants)
        .with_src(events)
        .with_config(configuration)
        .generate()
        .expect("generate the C header")
        .write(&mut output);
    String::from_utf8(output).unwrap()
}

fn definitions(calls: &[guest::GuestCall]) -> String {
    format!(
        "/* Generated from the graphics ABI declaration. Do not edit. */\n\
         #include <stddef.h>\n#include <stdint.h>\n\n\
         #include \"stubs.h\"\n#include \"xila/graphics.h\"\n\n\
         /* A color or style value as one call word. */\n\
         static size_t xila_graphics_pack(const void *value, size_t size) {{\n    \
         size_t word = 0;\n    __builtin_memcpy(&word, value, size);\n    return word;\n}}\n\
         {}",
        guest::c_definitions(calls, "xila_sdk_process")
    )
}

#[test]
fn committed_bindings_are_current() {
    let bindings = lvgl_rust_sys::_bindgen_raw_src();
    let catalog = abi::catalog(bindings).unwrap();
    let registry = abi::registry(&catalog).unwrap();
    let calls = guest::calls(&registry);
    let enumeration = enumeration::generate_calls(registry.calls()).to_string();
    let constants = guest_constants::generate(bindings).unwrap().to_string();
    let root = abi_directory();
    for (path, generated) in [
        (
            "sdk/rust/xila-sys/src/graphics/generated.rs",
            rust_module(&calls, &enumeration, &constants),
        ),
        (
            "include/xila/graphics.h",
            header(&calls, &enumeration, &constants),
        ),
        ("sdk/graphics.c", definitions(&calls)),
    ] {
        let committed = root.join(path);
        if std::env::var_os("XILA_UPDATE_HEADER").is_some() {
            fs::write(&committed, &generated).unwrap();
        }
        assert!(
            fs::read_to_string(&committed).is_ok_and(|text| text == generated),
            "{} is stale; regenerate with XILA_UPDATE_HEADER=1",
            committed.display()
        );
    }
}
