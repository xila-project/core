use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, quote};
use syn::{Ident, Signature};

use crate::{function::get_function_identifier, naming::snake_ident_to_upper_camel};

pub fn get_variant_identifier(identifier: &Ident) -> Ident {
    let identifier = get_function_identifier("", identifier);
    snake_ident_to_upper_camel(&identifier)
}

pub fn generate_code(signatures: Vec<Signature>) -> TokenStream {
    let catalog = crate::CallCatalog::from_signatures(signatures).expect("invalid graphics ABI");

    let variants = &catalog
        .calls()
        .iter()
        .map(|call| {
            let identifier = get_variant_identifier(&call.signature.ident);
            let value = Literal::usize_unsuffixed(call.id as usize);

            quote! { #identifier = #value }
        })
        .collect::<Vec<_>>();

    quote! {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        #[allow(dead_code)]
        #[repr(u16)]
        pub enum FunctionCall {
            #(
                #variants,
            )*
        }
    }
    .to_token_stream()
}
