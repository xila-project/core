use quote::format_ident;
use syn::{FnArg, Ident, Pat};

pub fn is_public_input(input: &&FnArg) -> bool {
    // If input ident start with two underscores, consider it private
    if let FnArg::Typed(pat_type) = input
        && let Pat::Ident(pat_ident) = &*pat_type.pat
    {
        return !pat_ident.ident.to_string().starts_with("__");
    }
    true
}

pub fn split_inputs<'a>(
    inputs: &'a [&'a FnArg],
) -> Result<(&'a [&'a FnArg], &'a [&'a FnArg]), String> {
    let index = inputs.iter().position(is_public_input).unwrap_or(0);

    Ok(inputs.split_at(index))
}

const NAMES: [(&str, &str); 1] = [("obj", "object")];

pub fn get_function_identifier(prefix: &str, identifier: &Ident) -> Ident {
    let identifier = identifier.to_string();
    let identifier = identifier.strip_prefix("lv_").unwrap_or(&identifier);

    // - Replace names
    let identifier = identifier
        .split("_")
        .map(|part| match NAMES.iter().find(|(old, _)| *old == part) {
            Some((_, new)) => new.to_string(),
            None => part.to_string(),
        })
        .collect::<Vec<String>>()
        .join("_");

    format_ident!("{}{}", prefix, identifier)
}
