use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, quote};
use syn::{Ident, Signature};

use crate::{Call, function::get_function_identifier, naming::snake_ident_to_upper_camel};

pub fn get_variant_identifier(identifier: &Ident) -> Ident {
    let identifier = get_function_identifier("", identifier);
    snake_ident_to_upper_camel(&identifier)
}

pub fn generate_code(signatures: Vec<Signature>) -> TokenStream {
    let catalog = crate::CallCatalog::from_signatures(signatures).expect("invalid graphics ABI");
    let calls: Vec<_> = catalog.calls().iter().collect();
    generate_calls(&calls)
}

/// Generate the guest enum from an already numbered catalog subset.
/// Unlike `generate_code`, this preserves the original catalog IDs.
pub fn generate_calls(calls: &[&Call]) -> TokenStream {
    let variants = &calls
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
