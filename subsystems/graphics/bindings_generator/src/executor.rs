//! The executor's call table, generated from the registry.
//!
//! The output is one `Function` enum with its ID, argument count and result
//! kind lookups, and one `run` function: a single `match` on the call that
//! either expands a scalar adapter inline, calls a named handler, or falls
//! through to the executor's handwritten match. Nothing is stored at runtime
//! beyond that code.
//!
//! The generated code is included by `xila-graphics` and refers to its
//! `handlers` module, `Kind` enum, `State`, `Transport`, `Call` and
//! `ResultKind` types.

use crate::{
    GeneratorError,
    enumeration::get_variant_identifier,
    registry::{AdapterPolicy, AutoObjectScalar, GraphicsRegistry, ObjectOperation},
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Generate the call table source for the executor from LVGL's own bindings.
pub fn source() -> Result<String, GeneratorError> {
    let catalog = crate::abi::catalog()?;
    let registry = crate::abi::registry(&catalog)?;
    Ok(generate(&registry)?.to_string())
}

pub fn generate(registry: &GraphicsRegistry<'_>) -> Result<TokenStream, GeneratorError> {
    // One row per call, sorted by ID: the ID and a byte holding the argument
    // count (low nibble) and result kind. Three bytes of read-only data per
    // call replace a `match` per property, each of which is a code table.
    let mut variants = Vec::new();
    let mut rows = Vec::new();
    for call in registry.calls() {
        let (_, arguments, result, _) = crate::abi::declaration(&call.name).ok_or_else(|| {
            GeneratorError::new(format!(
                "registry call {} has no ABI declaration",
                call.name
            ))
        })?;
        let variant = get_variant_identifier(&call.signature.ident);
        let id = i32::from(call.id);
        let kind: u8 = match *result {
            "None" => 0,
            "Word" => 1,
            "U32" => 2,
            "I32" => 3,
            other => {
                return Err(GeneratorError::new(format!(
                    "{}: unknown result kind {other}",
                    call.name
                )));
            }
        };
        variants.push(quote!(#variant = #id));
        rows.push((call.id, (kind << 4) | arguments.len() as u8));
    }
    rows.sort_by_key(|(id, _)| *id);
    let ids = rows.iter().map(|(id, _)| *id);
    let meta = rows.iter().map(|(_, meta)| *meta);
    let count = rows.len();
    let mut arms = Vec::new();
    for call in registry.calls() {
        let Some(policy) = registry.adapter_for(&call.name) else {
            continue;
        };
        let variant = get_variant_identifier(&call.signature.ident);
        let native = &call.signature.ident;
        // Xila's own widgets are declared without the `lv_` prefix.
        let native = if native.to_string().starts_with("lv_") {
            native.clone()
        } else {
            format_ident!("lv_{native}")
        };
        arms.push(match policy {
            AdapterPolicy::AutoObjectScalar(AutoObjectScalar::SetI32) => quote! {
                Function::#variant => handlers::object_set_i32(
                    state, call, None, |o, v| unsafe { crate::lvgl::#native(o, v) },
                )
            },
            AdapterPolicy::AutoObjectScalar(AutoObjectScalar::SetPairI32) => quote! {
                Function::#variant => handlers::object_set_i32_pair(
                    state, call, |o, x, y| unsafe { crate::lvgl::#native(o, x, y) },
                )
            },
            AdapterPolicy::AutoObjectScalar(AutoObjectScalar::GetI32) => quote! {
                Function::#variant => handlers::object_get_i32(
                    state, memory, call, None, |o| unsafe { crate::lvgl::#native(o) },
                )
            },
            AdapterPolicy::Object {
                kind,
                operation,
                native: renamed,
            } => {
                let native = renamed.map_or(native, |name| format_ident!("{name}"));
                let kind = match kind {
                    Some(kind) => {
                        let kind = format_ident!("{kind}");
                        quote!(Some(Kind::#kind))
                    }
                    None => quote!(None),
                };
                match operation {
                    ObjectOperation::Call => quote! {
                        Function::#variant => handlers::object_call(
                            state, call, #kind, |o| unsafe { crate::lvgl::#native(o) },
                        )
                    },
                    ObjectOperation::SetBool => quote! {
                        Function::#variant => handlers::object_set_bool(
                            state, call, #kind, |o, v| unsafe { crate::lvgl::#native(o, v) },
                        )
                    },
                    ObjectOperation::SetU8 => quote! {
                        Function::#variant => handlers::object_set_u8(
                            state, call, #kind, |o, v| unsafe { crate::lvgl::#native(o, v) },
                        )
                    },
                    ObjectOperation::SetU32 { min, max } => quote! {
                        Function::#variant => handlers::object_set_u32(
                            state, call, #kind, #min..=#max,
                            |o, v| unsafe { crate::lvgl::#native(o, v) },
                        )
                    },
                    ObjectOperation::SetMask { mask } => quote! {
                        Function::#variant => handlers::object_set_mask(
                            state, call, #kind, #mask,
                            |o, v| unsafe { crate::lvgl::#native(o, v) },
                        )
                    },
                    ObjectOperation::GetU32 => quote! {
                        Function::#variant => handlers::object_get_u32(
                            state, memory, call, #kind, |o| unsafe { crate::lvgl::#native(o) },
                        )
                    },
                    ObjectOperation::GetBool => quote! {
                        Function::#variant => handlers::object_get_u32(
                            state, memory, call, #kind,
                            |o| u32::from(unsafe { crate::lvgl::#native(o) }),
                        )
                    },
                    ObjectOperation::GetI32 => quote! {
                        Function::#variant => handlers::object_get_i32(
                            state, memory, call, #kind, |o| unsafe { crate::lvgl::#native(o) },
                        )
                    },
                }
            }
            AdapterPolicy::Create(kind) => {
                let kind = format_ident!("{kind}");
                quote! {
                    Function::#variant => {
                        handlers::create(state, memory, call, Kind::#kind, crate::lvgl::#native)
                    }
                }
            }
            AdapterPolicy::Custom(handler) => {
                let handler = format_ident!("{handler}");
                quote! { Function::#variant => handlers::#handler(state, memory, call) }
            }
            AdapterPolicy::Manual => continue,
        });
    }
    let maximum = crate::MAX_ARGUMENTS;
    Ok(quote! {
        /// The most argument words one call carries.
        pub const MAX_ARGUMENTS: usize = #maximum;

        /// Every call the executor implements, with its catalog ID.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(i32)]
        pub enum Function { #(#variants,)* }

        /// Call IDs in ascending order, and each call's argument count and
        /// result kind packed into a byte.
        static IDS: [u16; #count] = [#(#ids),*];
        static META: [u8; #count] = [#(#meta),*];

        impl Function {
            pub const fn id(self) -> i32 { self as i32 }

            /// The call with this ID, if any.
            pub fn from_id(id: i32) -> Option<Self> {
                Some(lookup(id)?.0)
            }
        }

        /// The call with this ID, its argument count and its result kind.
        pub(crate) fn lookup(id: i32) -> Option<(Function, i32, ResultKind)> {
            let index = IDS.binary_search(&u16::try_from(id).ok()?).ok()?;
            let result = match META[index] >> 4 {
                0 => ResultKind::None,
                1 => ResultKind::Word,
                2 => ResultKind::U32,
                _ => ResultKind::I32,
            };
            // SAFETY: `IDS` lists exactly the discriminants of `Function`, in
            // this table's generation, and `id` was found in it.
            let function = unsafe { core::mem::transmute::<i32, Function>(id) };
            Some((function, i32::from(META[index] & 0xf), result))
        }

        pub(crate) fn run(
            state: &mut State,
            memory: &mut dyn Transport,
            call: &Call,
            function: Function,
        ) -> Result<()> {
            match function {
                #(#arms,)*
                _ => handlers::manual(state, memory, call, function),
            }
        }
    })
}
