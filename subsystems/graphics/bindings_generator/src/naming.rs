pub fn snake_to_upper_camel_case(input: &str) -> String {
    input
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

pub fn snake_ident_to_upper_camel(ident: &syn::Ident) -> syn::Ident {
    syn::Ident::new(&snake_to_upper_camel_case(&ident.to_string()), ident.span())
}
