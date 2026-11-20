//! The guest API surface: every supported call as a typed function.
//!
//! One description of each call (its name, its arguments as the guest sees
//! them, how each becomes a call word, its result) feeds every guest
//! transport, so the Wasm and native guest APIs cannot differ in names or
//! types. This module builds that description from the registry and emits
//! the native transport's bindings: Rust functions, the C-ABI declarations a
//! header is generated from, and their C definitions. Hand-written code is
//! only the transport's raw call.

use crate::{
    enumeration::get_variant_identifier,
    function::{get_function_identifier, is_public_input},
    naming::snake_to_upper_camel_case,
    registry::GraphicsRegistry,
};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use std::fmt::Write;
use syn::{FnArg, Ident, ReturnType, Signature, Type};

/// Guest-visible types, as Rust source. Every transport's bindings include it.
pub const TYPES_SOURCE: &str = include_str!("guest_types.rs");

/// The prefix of every exported C name and of every C type.
pub const C_PREFIX: &str = "XilaGraphics";

/// How a guest value becomes one call word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Word {
    /// A scalar or pointer: converted as a number.
    Number,
    /// A word-sized register value whose bytes are copied, such as a color.
    Bytes,
}

/// One argument as the guest sees it.
pub struct Input {
    pub name: Ident,
    /// The guest type, with LVGL's names replaced by the guest API's.
    pub ty: Type,
    pub word: Word,
}

/// One call as the guest sees it.
pub struct GuestCall {
    /// The Rust function, which is the catalog name without LVGL's prefix.
    pub function: Ident,
    /// The call enum's variant.
    pub variant: Ident,
    pub inputs: Vec<Input>,
    /// The guest type of the result, if the call returns one.
    pub output: Option<Type>,
}

impl GuestCall {
    /// The C function name: `xila_graphics_` and the Rust name.
    pub fn c_function(&self) -> Ident {
        format_ident!("xila_graphics_{}", self.function)
    }

    fn c_variant(&self) -> String {
        let name = self.variant.to_string();
        let mut upper = String::new();
        for (index, character) in name.chars().enumerate() {
            if character.is_uppercase() && index != 0 {
                upper.push('_');
            }
            upper.push(character.to_ascii_uppercase());
        }
        format!("XILA_GRAPHICS_FUNCTION_CALL_{upper}")
    }
}

fn convert_type(mut ty: Type) -> Type {
    match &mut ty {
        Type::Path(path) => {
            if let Some(last) = path.path.segments.last_mut() {
                let identifier = match last.ident.to_string().as_str() {
                    "lv_obj_t" | "_lv_obj_t" => "Object".to_string(),
                    "lv_obj_class_t" => "ObjectClass".to_string(),
                    "lv_obj_flag_t" => "ObjectFlag".to_string(),
                    "lv_obj_point_transform_flag_t" => "ObjectPointTransformFlag".to_string(),
                    "lv_result_t" => "LvglResult".to_string(),
                    "WasmPointer" => "c_void".to_string(),
                    name => match name.strip_prefix("lv_") {
                        Some(name) => {
                            snake_to_upper_camel_case(name.strip_suffix("_t").unwrap_or(name))
                        }
                        None => name.to_string(),
                    },
                };
                last.ident = Ident::new(&identifier, last.ident.span());
            }
        }
        Type::Ptr(pointer) => *pointer.elem = convert_type((*pointer.elem).clone()),
        other => panic!("Unsupported argument type : {other:?}"),
    }
    ty
}

/// Handles are one word on the wire: `object_delete` takes 32 bits where the
/// catalog's C signature has 16.
fn widen_object_delete_handle(signature: &mut Signature) {
    for input in &mut signature.inputs {
        let FnArg::Typed(typed) = input else { continue };
        if !matches!(&*typed.pat, syn::Pat::Ident(pattern) if pattern.ident.to_string().starts_with("__"))
            && let Type::Path(path) = &*typed.ty
            && path.path.is_ident("u16")
        {
            *typed.ty = syn::parse_quote!(u32);
        }
    }
}

fn word(ty: &Type) -> Word {
    match ty {
        Type::Path(path) => match path.to_token_stream().to_string().as_str() {
            "lv_color_t" | "lv_color32_t" | "lv_color16_t" | "lv_style_value_t" => Word::Bytes,
            _ => Word::Number,
        },
        Type::Ptr(_) => Word::Number,
        other => panic!("Unsupported argument type : {other:?}"),
    }
}

/// The guest description of every call in the registry, in catalog order.
pub fn calls(registry: &GraphicsRegistry<'_>) -> Vec<GuestCall> {
    registry
        .calls()
        .iter()
        .map(|call| {
            let mut signature = call.signature.clone();
            if call.name == "object_delete" {
                widen_object_delete_handle(&mut signature);
            }
            let inputs = signature
                .inputs
                .iter()
                .filter(is_public_input)
                .map(|input| {
                    let FnArg::Typed(typed) = input else {
                        panic!("Unsupported argument type : {input:?}");
                    };
                    let syn::Pat::Ident(pattern) = &*typed.pat else {
                        panic!("Unsupported argument pattern : {:?}", typed.pat);
                    };
                    Input {
                        name: pattern.ident.clone(),
                        word: word(&typed.ty),
                        ty: convert_type((*typed.ty).clone()),
                    }
                })
                .collect();
            let output = match &signature.output {
                ReturnType::Type(_, ty) => Some(convert_type((**ty).clone())),
                ReturnType::Default => None,
            };
            GuestCall {
                function: get_function_identifier("", &signature.ident),
                variant: get_variant_identifier(&signature.ident),
                inputs,
                output,
            }
        })
        .collect()
}

/// Rust functions over a transport's `call::<T>(FunctionCall, &[usize])`,
/// which returns the call's result or its non-zero status.
pub fn rust_functions(calls: &[GuestCall]) -> TokenStream {
    let functions = calls.iter().map(|call| {
        let function = &call.function;
        let variant = &call.variant;
        let parameters = call.inputs.iter().map(|input| {
            let (name, ty) = (&input.name, &input.ty);
            quote! { #name: #ty }
        });
        let words = call.inputs.iter().map(|input| {
            let name = &input.name;
            match input.word {
                Word::Bytes => quote! { as_usize(#name) },
                Word::Number if is_usize(&input.ty) => quote! { #name },
                Word::Number => quote! { #name as usize },
            }
        });
        let output = match &call.output {
            Some(ty) => quote! { #ty },
            None => quote! { () },
        };
        quote! {
            /// # Safety
            /// Pointers must be valid for the call, and the call must run in a native process.
            pub unsafe fn #function(#(#parameters),*) -> Result<#output> {
                unsafe { call(FunctionCall::#variant, &[#(#words),*]) }
            }
        }
    });
    quote! { #(#functions)* }
}

fn is_usize(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.is_ident("usize"))
}

/// The C-ABI declarations a header generator reads: names and types only.
/// The bodies are never compiled.
pub fn c_declarations(calls: &[GuestCall]) -> TokenStream {
    let functions = calls.iter().map(|call| {
        let function = call.c_function();
        let mut parameters: Vec<_> = call
            .inputs
            .iter()
            .map(|input| {
                let (name, ty) = (&input.name, &input.ty);
                quote! { #name: #ty }
            })
            .collect();
        if let Some(ty) = &call.output {
            parameters.push(quote! { __result_pointer: *mut #ty });
        }
        quote! {
            #[unsafe(no_mangle)]
            pub extern "C" fn #function(#(#parameters),*) -> i32 { 0 }
        }
    });
    quote! { #(#functions)* }
}

fn c_type(ty: &Type) -> String {
    match ty {
        Type::Ptr(pointer) => {
            let target = c_type(&pointer.elem);
            if matches!(pointer.mutability, syn::PointerMutability::Const(_)) {
                format!("{target} const *")
            } else {
                format!("{target} *")
            }
        }
        Type::Path(path) => {
            let name = path
                .path
                .segments
                .last()
                .expect("a type has a name")
                .ident
                .to_string();
            match name.as_str() {
                "bool" => "bool".into(),
                "i8" => "int8_t".into(),
                "u8" => "uint8_t".into(),
                "u16" => "uint16_t".into(),
                "u32" => "uint32_t".into(),
                "i32" => "int32_t".into(),
                "usize" => "size_t".into(),
                "c_char" => "char".into(),
                "c_void" => "void".into(),
                _ => format!("{C_PREFIX}{name}"),
            }
        }
        other => panic!("Unsupported argument type : {other:?}"),
    }
}

/// The C definitions of [`c_declarations`]: each packs its arguments into
/// words and makes one call through the transport's raw `xila_graphics_call`,
/// whose first argument the transport names (`process`).
pub fn c_definitions(calls: &[GuestCall], process: &str) -> String {
    let mut source = String::new();
    for call in calls {
        let mut parameters: Vec<_> = call
            .inputs
            .iter()
            .map(|input| format!("{} {}", c_type(&input.ty), input.name))
            .collect();
        let result = match &call.output {
            Some(ty) => {
                parameters.push(format!("{} *__result_pointer", c_type(ty)));
                "__result_pointer"
            }
            None => "NULL",
        };
        let parameters = if parameters.is_empty() {
            "void".to_string()
        } else {
            parameters.join(", ")
        };
        let words: Vec<_> = call
            .inputs
            .iter()
            .map(|input| match input.word {
                Word::Bytes => format!("xila_graphics_pack(&{0}, sizeof({0}))", input.name),
                Word::Number => format!("(size_t){}", input.name),
            })
            .collect();
        writeln!(source, "\nint32_t {}({parameters}) {{", call.c_function())
            .expect("write to a string");
        let arguments = if words.is_empty() {
            "NULL".to_string()
        } else {
            writeln!(source, "    size_t arguments[] = {{{}}};", words.join(", "))
                .expect("write to a string");
            "arguments".to_string()
        };
        writeln!(
            source,
            "    return (int32_t)xila_graphics_call({process}, {}, {arguments}, {}, {result});\n}}",
            call.c_variant(),
            words.len(),
        )
        .expect("write to a string");
    }
    source
}
