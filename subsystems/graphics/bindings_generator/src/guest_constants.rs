//! Guest state/style constants discovered from the host's LVGL exports.
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Item;

pub fn generate(source: &str) -> syn::Result<TokenStream> {
    let file = syn::parse_file(source)?;
    let mut states = Vec::new();
    let mut types = Vec::new();
    for item in file.items {
        match item {
            Item::Const(item) => {
                let name = item.ident.to_string();
                let value = item.expr;
                if let Some(name) = name.strip_prefix("lv_state_t_LV_STATE_") {
                    let name = format_ident!("STATE_{name}");
                    states.push(quote! { pub const #name: State = #value; });
                }
            }
            Item::Type(item) => {
                let name = match item.ident.to_string().as_str() {
                    "lv_state_t" => Some("State"),
                    "lv_blur_quality_t" => Some("BlurQuality"),
                    "lv_text_leading_trim_t" => Some("TextLeadingTrim"),
                    _ => None,
                };
                if let Some(name) = name {
                    let name = format_ident!("{name}");
                    let ty = item.ty;
                    types.push(quote! { pub type #name = #ty; });
                }
            }
            _ => {}
        }
    }
    Ok(quote! {
        #(#types)*
        #(#states)*
    })
}
