use std::ops::Deref;
use std::path::Path;

use crate::utilities::enumeration;
use crate::utilities::file::write_token_stream_to_file;
use crate::utilities::format::snake_to_upper_camel_case;
use crate::utilities::function::{get_function_identifier, is_public_input};
#[cfg(feature = "c_bindings")]
use cbindgen::{EnumConfig, ExportConfig, FunctionConfig, RenameRule};
use graphics_bindgen::{Call, registry::GraphicsRegistry};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{FnArg, Ident, ReturnType, Signature, Type};

fn convert_type(mut ty: Type) -> Type {
    match &mut ty {
        Type::Path(path) => {
            if let Some(last_segment) = path.path.segments.last_mut() {
                let new_ident = match last_segment.ident.to_string().as_str() {
                    "lv_obj_t" | "_lv_obj_t" => format_ident!("Object"),
                    "lv_obj_class_t" => format_ident!("ObjectClass"),
                    "lv_obj_flag_t" => format_ident!("ObjectFlag"),
                    "lv_obj_point_transform_flag_t" => format_ident!("ObjectPointTransformFlag"),
                    "lv_result_t" => format_ident!("LvglResult"),
                    "WasmPointer" => format_ident!("c_void"),
                    identifier => {
                        let ident = if identifier.starts_with("lv_") {
                            let ident = identifier.strip_prefix("lv_").unwrap_or(identifier);
                            let ident = ident.strip_suffix("_t").unwrap_or(ident);
                            snake_to_upper_camel_case(ident)
                        } else {
                            identifier.to_string()
                        };

                        Ident::new(&ident, last_segment.ident.span())
                    }
                };

                last_segment.ident = new_ident;
            }
        }
        Type::Ptr(pointer_type) => {
            *pointer_type.elem = convert_type(pointer_type.elem.deref().clone());
        }
        r#type => panic!("Unsupported argument type : {type:?}"),
    }
    ty
}

fn convert_function_argument(input: &&syn::FnArg) -> TokenStream {
    match input {
        FnArg::Typed(pattern_type) => {
            let pattern = &pattern_type.pat;
            let ty = convert_type(pattern_type.ty.deref().clone());

            quote! {
                #pattern : #ty
            }
        }
        receiver => panic!("Unsupported argument type : {receiver:?}"),
    }
}

fn get_passed_input(input: &&syn::FnArg) -> TokenStream {
    match input {
        FnArg::Typed(pattern_type) => match &*pattern_type.ty {
            Type::Path(path) => {
                let pattern = &pattern_type.pat;
                let pattern_string = path.to_token_stream().to_string();
                if pattern_string == "lv_color_t"
                    || pattern_string == "lv_color32_t"
                    || pattern_string == "lv_color16_t"
                    || pattern_string == "lv_style_value_t"
                {
                    quote! {
                        as_usize( #pattern )
                    }
                } else if pattern_string == "usize" {
                    quote! {
                        #pattern
                    }
                } else {
                    quote! {
                        #pattern as usize
                    }
                }
            }
            Type::Ptr(_) => {
                let pattern = &pattern_type.pat;
                quote! {
                    #pattern as usize
                }
            }
            r#type => panic!("Unsupported argument type : {type:?}"),
        },
        receiver => panic!("Unsupported argument type : {receiver:?}"),
    }
}

fn generate_xila_graphics_call(signature: &Signature) -> TokenStream {
    let filtered_inputs = signature
        .inputs
        .iter()
        .filter(is_public_input)
        .collect::<Vec<_>>();

    let enumeration_variant = enumeration::get_variant_identifier(&signature.ident);

    let mut passed_arguments = filtered_inputs
        .iter()
        .map(get_passed_input)
        .collect::<Vec<_>>();

    let remaining_arguments = graphics_bindgen::MAX_ARGUMENTS - passed_arguments.len();
    for _ in 0..remaining_arguments {
        passed_arguments.push(quote! { 0 });
    }

    let argument_count = filtered_inputs.len() as u8;

    let passed_result = if let ReturnType::Type(..) = &signature.output {
        quote! {
            __result_pointer as *mut _ as *mut core::ffi::c_void
        }
    } else {
        quote! { core::ptr::null_mut() }
    };

    quote! {
        xila_graphics_call(
                    FunctionCall::#enumeration_variant,
                    #( #passed_arguments ),*,
                    #argument_count,
                    #passed_result
                )
    }
}

#[cfg(feature = "c_bindings")]
fn generate_c_function(signature: &Signature) -> TokenStream {
    let filtered_inputs = signature
        .inputs
        .iter()
        .filter(is_public_input)
        .collect::<Vec<_>>();

    let mut inputs = filtered_inputs
        .iter()
        .map(convert_function_argument)
        .collect::<Vec<_>>();

    let function_identifier = get_function_identifier("xila_graphics_", &signature.ident);

    if let ReturnType::Type(_, type_value) = &signature.output {
        let result_argument_type = convert_type(type_value.deref().clone());
        let result_argument = quote! {
            __result_pointer: *mut #result_argument_type
        };

        inputs.push(result_argument);
    }

    let generated_xila_graphics_call = generate_xila_graphics_call(signature);

    quote! {
        #[unsafe(no_mangle)]
        pub extern "C" fn #function_identifier(
            #( #inputs ),*
        ) -> i32 {
            unsafe {
                #generated_xila_graphics_call
            }
        }
    }
}

fn generate_function(signature: &Signature) -> TokenStream {
    let function_identifier = get_function_identifier("", &signature.ident);

    let filtered_inputs = signature
        .inputs
        .iter()
        .filter(is_public_input)
        .collect::<Vec<_>>();

    let inputs = filtered_inputs
        .iter()
        .map(convert_function_argument)
        .collect::<Vec<_>>();

    let result_data_type = if let ReturnType::Type(_, type_value) = &signature.output {
        let type_value = convert_type(type_value.deref().clone());

        quote! {
            #type_value
        }
    } else {
        quote! { () }
    };

    let result_data_expression = quote! {
        let mut __result_data: #result_data_type = unsafe { core::mem::zeroed() };
        let __result_pointer = &mut __result_data as *mut _;
    };

    let generated_xila_graphics_call = generate_xila_graphics_call(signature);

    quote! {
        /// # Safety
        /// This function is unsafe because it may dereference raw pointers.
        pub unsafe fn #function_identifier(
            #( #inputs ),*
        ) -> Result<#result_data_type> {

            #result_data_expression

            let __result_status =
            unsafe {
                #generated_xila_graphics_call
            };

            if __result_status == 0 {
                Ok(__result_data)
            } else {
                Err(__result_status)
            }

        }
    }
}

fn generate_enumeration(path: impl AsRef<Path>, calls: &[&Call]) -> Result<(), String> {
    let enumeration = enumeration::generate_calls(calls);
    let constants = graphics_bindgen::guest_constants::generate(lvgl_rust_sys::_bindgen_raw_src())
        .map_err(|error| error.to_string())?;
    let token_stream = quote! {
        #enumeration
        #constants
    };

    write_token_stream_to_file(path, token_stream)?;

    Ok(())
}

fn generate_functions(path: impl AsRef<Path>, signatures: &[Signature]) -> Result<(), String> {
    let generated_functions = signatures.iter().map(generate_function).collect::<Vec<_>>();

    let token_stream = quote! {
       use crate::prelude::*;

       #( #generated_functions )*
    };

    write_token_stream_to_file(path, token_stream)?;

    Ok(())
}

#[cfg(feature = "c_bindings")]
fn generate_c_abi_functions(
    path: impl AsRef<Path>,
    signatures: &[Signature],
) -> Result<(), String> {
    let generated_c_abi_functions = signatures
        .iter()
        .map(generate_c_function)
        .collect::<Vec<_>>();

    let token_stream = quote! {
       use crate::prelude::*;

        #( #generated_c_abi_functions )*
    };

    println!(
        "cargo:warning=Generating C ABI functions at path: {}",
        path.as_ref().display()
    );

    write_token_stream_to_file(path, token_stream)?;

    Ok(())
}

pub fn generate(output_path: &Path, registry: &GraphicsRegistry<'_>) {
    let calls = registry.calls();
    let signatures: Vec<_> = calls.iter().map(|call| call.signature.clone()).collect();

    let enumerations_generated_path = output_path.join("enumeration.generated.rs");
    let functions_generated_path = output_path.join("functions.generated.rs");

    generate_enumeration(&enumerations_generated_path, calls).unwrap();

    generate_functions(&functions_generated_path, &signatures).unwrap();

    #[cfg(feature = "c_bindings")]
    {
        let c_functions_generated_path = output_path.join("c_functions.generated.rs");
        let c_header_path = output_path.join("xila_graphics.h");
        generate_c_abi_functions(&c_functions_generated_path, &signatures).unwrap();

        let configuration: cbindgen::Config = cbindgen::Config {
            language: cbindgen::Language::C,
            include_guard: Some("__XILA_GRAPHICS_GENERATED_H_INCLUDED".to_string()),
            sys_includes: vec![
                "stdarg.h".to_string(),
                "stdbool.h".to_string(),
                "stdint.h".to_string(),
            ],
            export: ExportConfig {
                prefix: Some("XilaGraphics".to_string()),
                ..Default::default()
            },
            function: FunctionConfig {
                ..Default::default()
            },
            no_includes: true,
            enumeration: EnumConfig {
                rename_variants: RenameRule::QualifiedScreamingSnakeCase,
                ..Default::default()
            },
            ..Default::default()
        };

        cbindgen::Builder::new()
            // Parse expanded inputs directly, without temporarily overwriting
            // source files or recursively enabling this crate's host defaults.
            .with_src(&c_functions_generated_path)
            .with_src(&enumerations_generated_path)
            .with_src(
                std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
                    .join("src/graphics_event.rs"),
            )
            .with_src(
                std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
                    .join("src/guest/graphics/prelude.rs"),
            )
            .with_config(configuration)
            .generate()
            .expect("Unable to generate bindings")
            .write_to_file(&c_header_path);
    }
}
