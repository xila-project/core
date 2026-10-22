use crate::{MAX_ARGUMENTS, context::LvglContext, function::get_function_identifier};
use quote::ToTokens;
use std::{collections::BTreeSet, fmt};
use syn::{FnArg, Pat, ReturnType, Signature, Type};

// Append-only IDs for new Xila helpers. Inserting these into the legacy
// alphabetical table would renumber existing guest calls, including Calculator.
fn extension_id(name: &str) -> Option<u16> {
    Some(match name {
        "indexed_canvas_create" => 0xf000,
        "indexed_canvas_set_pixels" => 0xf001,
        "window_pop_pointer_event" => 0xf002,
        "keyboard_create" => 0xf003,
        "keyboard_set_textarea" => 0xf004,
        "object_set_padding" => 0xf005,
        "grid_fr" => 0xf006,
        "object_set_background" => 0xf007,
        "object_set_text_color" => 0xf008,
        "object_set_border" => 0xf009,
        "object_set_radius" => 0xf00a,
        "object_set_text_align" => 0xf00b,
        "object_set_font" => 0xf00c,
        "object_set_surface" => 0xf00d,
        "object_set_min_max_size" => 0xf00e,
        "bar_create" => 0xf00f,
        "switch_create" => 0xf010,
        "spinner_create" => 0xf011,
        "range_set_range" => 0xf012,
        "range_set_value" => 0xf013,
        "range_get_value" => 0xf014,
        "range_get_start_value" => 0xf015,
        "range_set_mode" => 0xf016,
        "range_set_orientation" => 0xf017,
        "spinner_set_animation" => 0xf018,
        "spinner_get_animation_duration" => 0xf019,
        "spinner_get_arc_sweep" => 0xf01a,
        "dropdown_get_selected_string" => 0xf01b,
        "roller_get_selected_string" => 0xf01c,
        "dropdown_get_highlight" => 0xf01d,
        "dropdown_is_open_state" => 0xf01e,
        "calendar_set_today" => 0xf01f,
        "calendar_set_highlights" => 0xf020,
        "calendar_get_shown_date" => 0xf021,
        "chart_add_series_slot" => 0xf022,
        "chart_set_values" => 0xf023,
        "chart_get_value" => 0xf024,
        "list_create" => 0xf025,
        "list_add_text" => 0xf026,
        "list_add_text_button" => 0xf027,
        "list_set_button_text" => 0xf028,
        _ => return None,
    })
}

#[derive(Debug)]
pub struct GeneratorError(String);

impl GeneratorError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for GeneratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for GeneratorError {}

#[derive(Debug)]
pub struct Call {
    pub id: u16,
    pub name: String,
    pub signature: Signature,
    pub argument_count: usize,
    pub host_arguments: Vec<String>,
}

impl Call {
    /// Ensure a handwritten adapter still agrees with the discovered C ABI.
    /// Context-only helper arguments are not part of the guest contract.
    pub fn validate_signature(
        &self,
        arguments: &[&str],
        output: Option<&str>,
    ) -> Result<(), GeneratorError> {
        fn normalize(text: &str) -> String {
            text.chars().filter(|c| !c.is_whitespace()).collect()
        }
        let actual: Vec<_> = self
            .signature
            .inputs
            .iter()
            .filter_map(|arg| {
                let FnArg::Typed(arg) = arg else { return None };
                let Pat::Ident(name) = &*arg.pat else {
                    return None;
                };
                if name.ident.to_string().starts_with("__") {
                    return None;
                }
                Some(normalize(&arg.ty.to_token_stream().to_string()))
            })
            .collect();
        let result = match &self.signature.output {
            ReturnType::Default => None,
            ReturnType::Type(_, ty) => Some(normalize(&ty.to_token_stream().to_string())),
        };
        if self.signature.variadic.is_some()
            || actual
                != arguments
                    .iter()
                    .map(|arg| normalize(arg))
                    .collect::<Vec<_>>()
            || result != output.map(normalize)
        {
            return Err(GeneratorError(format!(
                "adapter signature changed for {}",
                self.name
            )));
        }
        Ok(())
    }

    /// Conservative policy: pointer lifetimes cannot be inferred from a C signature.
    /// Strings, arrays, native structures and unions need explicit adapters.
    pub fn generic_safe(&self) -> bool {
        fn safe_type(ty: &Type) -> bool {
            match ty {
                Type::Path(path) => {
                    let name = path.to_token_stream().to_string();
                    matches!(
                        name.as_str(),
                        "u8" | "u16"
                            | "u32"
                            | "u64"
                            | "i8"
                            | "i16"
                            | "i32"
                            | "i64"
                            | "bool"
                            | "usize"
                            | "isize"
                            | "lv_coord_t"
                            | "lv_state_t"
                            | "lv_obj_flag_t"
                            | "lv_style_selector_t"
                            | "lv_flex_flow_t"
                            | "lv_opa_t"
                    )
                }
                Type::Ptr(ptr) => ptr.elem.to_token_stream().to_string() == "lv_obj_t",
                _ => false,
            }
        }
        self.signature.variadic.is_none()
            && self.signature.inputs.iter().all(|arg| {
                let FnArg::Typed(arg) = arg else { return false };
                let Pat::Ident(name) = &*arg.pat else {
                    return false;
                };
                !name.ident.to_string().starts_with("__") && safe_type(&arg.ty)
            })
            && match &self.signature.output {
                ReturnType::Default => true,
                ReturnType::Type(_, ty) => safe_type(ty),
            }
    }
}

#[derive(Debug)]
pub struct CallCatalog {
    calls: Vec<Call>,
}

impl CallCatalog {
    pub fn parse(source: &str) -> Result<Self, GeneratorError> {
        let context =
            LvglContext::parse(source).map_err(|error| GeneratorError(error.to_string()))?;
        Self::from_signatures(context.get_signatures())
    }

    pub fn from_signatures(mut signatures: Vec<Signature>) -> Result<Self, GeneratorError> {
        signatures.sort_by_key(|sig| {
            let name = sig.ident.to_string();
            (extension_id(&name).is_some(), name.to_lowercase())
        });
        let mut names = BTreeSet::new();
        let mut legacy_id = 0u16;
        let calls = signatures
            .into_iter()
            .map(|signature| {
                let id = if let Some(id) = extension_id(&signature.ident.to_string()) {
                    id
                } else {
                    if legacy_id >= 0xf000 {
                        return Err(GeneratorError("too many legacy call IDs".into()));
                    }
                    let id = legacy_id;
                    legacy_id += 1;
                    id
                };
                let name = get_function_identifier("", &signature.ident).to_string();
                if !names.insert(name.clone()) {
                    return Err(GeneratorError(format!("duplicate guest call {name}")));
                }
                let mut argument_count = 0;
                let mut host_arguments = Vec::new();
                for argument in &signature.inputs {
                    let FnArg::Typed(argument) = argument else {
                        return Err(GeneratorError(format!("receiver in {name}")));
                    };
                    let Pat::Ident(ident) = &*argument.pat else {
                        return Err(GeneratorError(format!("unnamed argument in {name}")));
                    };
                    let ident = ident.ident.to_string();
                    if ident.starts_with("__") {
                        host_arguments.push(ident);
                    } else {
                        argument_count += 1;
                    }
                }
                if argument_count > MAX_ARGUMENTS {
                    return Err(GeneratorError(format!(
                        "{name} exceeds seven guest arguments"
                    )));
                }
                Ok(Call {
                    id,
                    name,
                    signature,
                    argument_count,
                    host_arguments,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { calls })
    }

    pub fn calls(&self) -> &[Call] {
        &self.calls
    }

    /// Select a registry subset by normalized call name without changing catalog IDs.
    pub fn select(&self, names: &[&str]) -> Result<Vec<&Call>, GeneratorError> {
        let mut selected = Vec::with_capacity(names.len());
        let mut unique = BTreeSet::new();
        for name in names {
            if !unique.insert(*name) {
                return Err(GeneratorError(format!("duplicate supported call {name}")));
            }
            let call = self
                .calls
                .iter()
                .find(|call| call.name == *name)
                .ok_or_else(|| {
                    GeneratorError(format!("supported call {name} is not in the LVGL catalog"))
                })?;
            selected.push(call);
        }
        Ok(selected)
    }

    pub fn signatures(&self) -> Vec<Signature> {
        self.calls
            .iter()
            .map(|call| call.signature.clone())
            .collect()
    }
}
