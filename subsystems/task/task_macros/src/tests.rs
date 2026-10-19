use super::TaskArguments;
use darling::{FromMeta, ast::NestedMeta};
use quote::{ToTokens, quote};

#[test]
fn empty_arguments_use_default_task_path_and_executor() {
    let arguments = TaskArguments::from_list(&[]).unwrap();

    assert_eq!(arguments.task_path.to_token_stream().to_string(), "task");
    assert_eq!(
        arguments.executor.to_token_stream().to_string(),
        "drivers_std :: executor :: instantiate_static_executor ! ()"
    );
}

#[test]
fn task_path_override_preserves_default_executor() {
    let meta = NestedMeta::parse_meta_list(quote!(task_path = xila::task)).unwrap();
    let arguments = TaskArguments::from_list(&meta).unwrap();

    assert_eq!(
        arguments.task_path.to_token_stream().to_string(),
        "xila :: task"
    );
    assert_eq!(
        arguments.executor.to_token_stream().to_string(),
        "drivers_std :: executor :: instantiate_static_executor ! ()"
    );
}

#[test]
fn executor_override_accepts_macro_expression() {
    let meta = NestedMeta::parse_meta_list(quote!(
        task_path = xila::task,
        executor = drivers_wasm::executor::instantiate_static_executor!()
    ))
    .unwrap();
    let arguments = TaskArguments::from_list(&meta).unwrap();

    assert_eq!(
        arguments.task_path.to_token_stream().to_string(),
        "xila :: task"
    );
    assert_eq!(
        arguments.executor.to_token_stream().to_string(),
        "drivers_wasm :: executor :: instantiate_static_executor ! ()"
    );
}
