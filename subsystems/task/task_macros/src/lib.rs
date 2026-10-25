use darling::{FromMeta, ast::NestedMeta};
use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{ItemFn, parse_macro_input, parse_str};

#[cfg(test)]
mod tests;

fn default_task_path() -> syn::Expr {
    parse_str("task").unwrap()
}

fn default_executor() -> syn::Expr {
    parse_str("drivers_std::executor::instantiate_static_executor!()").unwrap()
}

#[derive(Debug, FromMeta, Clone)]
struct TaskArguments {
    #[darling(default = "default_task_path")]
    pub task_path: syn::Expr,

    #[darling(default = "default_executor")]
    pub executor: syn::Expr,

    #[darling(default)]
    pub manager: Option<syn::Expr>,
}

impl TaskArguments {
    fn from_token_stream(arguments: TokenStream) -> Result<Self, darling::Error> {
        let arguments = NestedMeta::parse_meta_list(arguments.into()).unwrap();
        Self::from_list(&arguments.clone())
    }
}

/// A procedural macro to annotate test functions.
///
/// This macro wraps the annotated async function to be executed in a blocking context
/// using embassy_futures::block_on, similar to how other modules handle async operations.
///
/// # Requirements
///
/// Test functions must:
/// - Be async
/// - Have no arguments
/// - Have no return type (or return unit type `()`)
#[proc_macro_attribute]
pub fn test(arguments: TokenStream, input: TokenStream) -> TokenStream {
    let input_function = parse_macro_input!(input as ItemFn);
    let function_name = &input_function.sig.ident;

    // check if arch is not linux, windows or macos
    if !cfg!(any(
        target_os = "linux",
        target_os = "windows",
        target_os = "macos"
    )) {
        return quote! {
            #[std::prelude::v1::test]
            fn #function_name() {
                println!("Test {} is ignored on this platform.", stringify!(#function_name));
            }
        }
        .to_token_stream()
        .into();
    }

    let arguments = match TaskArguments::from_token_stream(arguments) {
        Ok(o) => o,
        Err(e) => return e.write_errors().into(),
    };

    let executor = arguments.executor;
    let task_path = arguments.task_path;

    // Extract function details
    let function_name = &input_function.sig.ident;

    let function_name_string = function_name.to_string();
    // Tests use one serialized manager so `testing::initialize` and the test
    // task share the same manager regardless of how the task crate is named.
    let manager_setup = quote! {
        let _test_lock = #task_path::test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let manager: &'static #task_path::Manager = #task_path::reset_test_manager();
    };

    // Check if function is async
    let is_asynchronous = input_function.sig.asyncness.is_some();

    if !is_asynchronous {
        return syn::Error::new_spanned(
            input_function.sig.fn_token,
            "Test functions must be async",
        )
        .to_compile_error()
        .into();
    }

    // Check if function has no arguments
    if !input_function.sig.inputs.is_empty() {
        return syn::Error::new_spanned(
            &input_function.sig.inputs,
            "Test functions must not have any arguments",
        )
        .to_compile_error()
        .into();
    }

    // Check if function has no return type (or returns unit type)
    if let syn::ReturnType::Type(_, return_type) = &input_function.sig.output {
        // Allow unit type () but reject any other return type
        if let syn::Type::Tuple(tuple) = return_type.as_ref() {
            if !tuple.elems.is_empty() {
                return syn::Error::new_spanned(
                    return_type,
                    "Test functions must not have a return type",
                )
                .to_compile_error()
                .into();
            }
        } else {
            return syn::Error::new_spanned(
                return_type,
                "Test functions must not have a return type",
            )
            .to_compile_error()
            .into();
        }
    }

    // Extract attributes to preserve them on the outer function
    let attributes = &input_function.attrs;

    // Change ident to __inner to avoid name conflicts
    let mut input_function = input_function.clone();
    input_function.sig.ident = format_ident!("__inner");
    input_function.attrs.clear(); // Remove attributes from inner function

    // Generate the new function
    quote! {
        #(#attributes)*
        #[std::prelude::v1::test]
        fn #function_name() {
            #input_function

            static mut __SPAWNER : usize = 0;
            #manager_setup

            unsafe {
                let __EXECUTOR = #executor;

                __EXECUTOR.run(|Spawner, __executor| {
                    unsafe {
                        __SPAWNER = manager.register_spawner(Spawner).expect("Failed to register spawner");
                    }

                    #task_path::block_on(async move {
                        manager.spawn(
                            #task_path::Manager::ROOT_TASK_IDENTIFIER,
                            #function_name_string,
                            Some(__SPAWNER),
                            async move |_task| {
                                __inner().await;
                                __executor.stop();
                            }
                        ).await
                    }).expect("Failed to spawn task");
                });
            }
            unsafe {
                manager.unregister_spawner(__SPAWNER).expect("Failed to unregister spawner");
            }

        }
    }
    .to_token_stream()
    .into()
}

/// A procedural macro to annotate functions that should run with a specific executor.
///
/// This macro wraps the annotated async function to be executed with a provided
/// executor, handling the registration, spawning, and cleanup automatically.
///
/// # Requirements
///
/// Functions must:
/// - Be async
/// - Accept one `&'static Manager` argument
/// - Have no return type (or return unit type `()`)
///
/// # Usage
///
/// The macro accepts an executor expression and a reference to the manager
/// owned by the composition root:
///
/// ```rust,ignore
/// #[task_macros::run(
///     executor = drivers_std::executor::instantiate_static_executor!(),
///     manager = &TASK_MANAGER
/// )]
/// async fn my_function(task_manager: &'static task::Manager) {
///     println!("Running with custom executor!");
/// }
/// ```
#[proc_macro_attribute]
pub fn run(arguments: TokenStream, input: TokenStream) -> TokenStream {
    let arguments = match TaskArguments::from_token_stream(arguments) {
        Ok(o) => o,
        Err(e) => return e.write_errors().into(),
    };
    let input_function = parse_macro_input!(input as ItemFn);

    let task_path = arguments.task_path;
    let executor_expression = arguments.executor;
    let Some(manager_expression) = arguments.manager else {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "run requires `manager = <expression>`",
        )
        .to_compile_error()
        .into();
    };

    // Extract function details
    let function_name = &input_function.sig.ident;
    let function_name_string = function_name.to_string();
    let manager_setup = quote! {
        let manager: &'static #task_path::Manager = #manager_expression;
    };

    // Check if function is async
    let is_asynchronous = input_function.sig.asyncness.is_some();

    if !is_asynchronous {
        return syn::Error::new_spanned(
            input_function.sig.fn_token,
            "Functions with Run_with_executor must be async",
        )
        .to_compile_error()
        .into();
    }

    // The run manager owns the executor spawner and must also be used by the
    // async entry point for any tasks it spawns.
    if input_function.sig.inputs.len() != 1 {
        return syn::Error::new_spanned(
            &input_function.sig.inputs,
            "Functions with Run_with_executor must accept one &'static Manager argument",
        )
        .to_compile_error()
        .into();
    }

    // Check if function has no return type (or returns unit type)
    if let syn::ReturnType::Type(_, return_type) = &input_function.sig.output {
        // Allow unit type () but reject any other return type
        if let syn::Type::Tuple(tuple) = return_type.as_ref() {
            if !tuple.elems.is_empty() {
                return syn::Error::new_spanned(
                    return_type,
                    "Functions with Run_with_executor must not have a return type",
                )
                .to_compile_error()
                .into();
            }
        } else {
            return syn::Error::new_spanned(
                return_type,
                "Functions with Run_with_executor must not have a return type",
            )
            .to_compile_error()
            .into();
        }
    }

    // Extract attributes to preserve them on the outer function
    let attributes = &input_function.attrs;

    // Change ident to __inner to avoid name conflicts
    let mut input_function = input_function.clone();
    input_function.sig.ident = format_ident!("__inner");
    input_function.attrs.clear(); // Remove attributes from inner function

    // Generate the new function
    quote! {
        #(#attributes)*
        fn #function_name() {
            #input_function

            static mut __SPAWNER : usize = 0;
            #manager_setup

            unsafe {
                let __EXECUTOR : &'static mut _ = #executor_expression;

                __EXECUTOR.start(|Spawner| {
                    unsafe {
                        __SPAWNER = manager.register_spawner(Spawner).expect("Failed to register spawner");
                    }

                    #task_path::block_on(async move {
                        manager.spawn(
                            #task_path::Manager::ROOT_TASK_IDENTIFIER,
                            #function_name_string,
                            Some(__SPAWNER),
                            async move |_task| {
                                __inner(manager).await;
                            }
                        ).await
                    }).expect("Failed to spawn task");
                });
            }
        }
    }
    .to_token_stream()
    .into()
}
