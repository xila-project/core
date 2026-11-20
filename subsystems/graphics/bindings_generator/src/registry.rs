//! Build-time support registry for the WASM graphics ABI.
//!
//! This stores no runtime metadata. The build script uses the selected calls
//! and adapter policies to emit Rust code and discard the registry.

use crate::{Call, CallCatalog, GeneratorError};
use quote::ToTokens;
use std::collections::{BTreeMap, BTreeSet};
use syn::ReturnType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoObjectScalar {
    SetI32,
    SetPairI32,
    GetI32,
}

/// What a generated object call does with its argument words. The object is
/// always the first word, resolved (and checked against the rule's kind)
/// before anything else is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectOperation {
    /// `(object)`.
    Call,
    /// `(object, bool)`; a word above 1 is invalid.
    SetBool,
    /// `(object, u8)`.
    SetU8,
    /// `(object, u32)`, the word within `min..=max`.
    SetU32 { min: u32, max: u32 },
    /// `(object, u32)`, a word made of the bits in `mask` only.
    SetMask { mask: u32 },
    /// `(object) -> u32`.
    GetU32,
    /// `(object) -> bool`, widened to a `u32` result.
    GetBool,
    /// `(object) -> i32`.
    GetI32,
}

impl ObjectOperation {
    /// Arguments after the object, and whether the call returns a value.
    fn shape(self) -> (usize, bool) {
        match self {
            Self::Call => (0, false),
            Self::SetBool | Self::SetU8 | Self::SetU32 { .. } | Self::SetMask { .. } => (1, false),
            Self::GetU32 | Self::GetBool | Self::GetI32 => (0, true),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterPolicy {
    /// Existing handwritten dispatch arm; used until it is migrated.
    Manual,
    /// Generated scalar call. Object arguments are explicitly guest handles.
    AutoObjectScalar(AutoObjectScalar),
    /// Generated dispatch arm calls this named handwritten adapter.
    Custom(&'static str),
    /// Generated call on one object of an optional kind (`None`: any object).
    /// The native function is called through a shared helper per operation.
    Object {
        kind: Option<&'static str>,
        operation: ObjectOperation,
        /// The LVGL function, when it is not the call's own name with `lv_`.
        native: Option<&'static str>,
    },
    /// Generated constructor: the native `(parent) -> object` function plus the
    /// kind the new object is registered under.
    Create(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct AdapterRule {
    name: &'static str,
    policy: AdapterPolicy,
}

impl AdapterRule {
    pub const fn automatic(name: &'static str, adapter: AutoObjectScalar) -> Self {
        Self {
            name,
            policy: AdapterPolicy::AutoObjectScalar(adapter),
        }
    }

    pub const fn object(
        name: &'static str,
        kind: Option<&'static str>,
        operation: ObjectOperation,
        native: Option<&'static str>,
    ) -> Self {
        Self {
            name,
            policy: AdapterPolicy::Object {
                kind,
                operation,
                native,
            },
        }
    }

    pub const fn create(name: &'static str, kind: &'static str) -> Self {
        Self {
            name,
            policy: AdapterPolicy::Create(kind),
        }
    }

    pub const fn custom(name: &'static str, handler: &'static str) -> Self {
        Self {
            name,
            policy: AdapterPolicy::Custom(handler),
        }
    }
}

pub struct GraphicsRegistry<'a> {
    calls: Vec<&'a Call>,
    adapters: BTreeMap<String, AdapterPolicy>,
}

impl<'a> GraphicsRegistry<'a> {
    pub fn new(
        catalog: &'a CallCatalog,
        supported_names: &[&str],
        rules: &[AdapterRule],
    ) -> Result<Self, GeneratorError> {
        let calls = catalog.select(supported_names)?;
        let supported: BTreeSet<_> = calls.iter().map(|call| call.name.as_str()).collect();
        let mut adapters: BTreeMap<_, _> = calls
            .iter()
            .map(|call| (call.name.clone(), AdapterPolicy::Manual))
            .collect();
        let mut seen_rules = BTreeSet::new();

        for rule in rules {
            if !supported.contains(rule.name) {
                return Err(GeneratorError::new(format!(
                    "adapter policy for unregistered call {}",
                    rule.name
                )));
            }
            if !seen_rules.insert(rule.name) {
                return Err(GeneratorError::new(format!(
                    "duplicate adapter policy for {}",
                    rule.name
                )));
            }
            adapters.insert(rule.name.to_string(), rule.policy);
            if let AdapterPolicy::AutoObjectScalar(adapter) = rule.policy {
                let call = calls
                    .iter()
                    .find(|call| call.name == rule.name)
                    .expect("validated supported name");
                validate_auto_object_scalar(call, adapter)?;
            }
            if let AdapterPolicy::Object { operation, .. } = rule.policy {
                let call = calls
                    .iter()
                    .find(|call| call.name == rule.name)
                    .expect("validated supported name");
                let (extra, returns) = operation.shape();
                if !call.generic_safe()
                    || call.argument_count != 1 + extra
                    || matches!(call.signature.output, ReturnType::Type(..)) != returns
                {
                    return Err(GeneratorError::new(format!(
                        "object adapter for {} does not match its signature",
                        rule.name
                    )));
                }
            }
            if let AdapterPolicy::Create(_) = rule.policy {
                let call = calls
                    .iter()
                    .find(|call| call.name == rule.name)
                    .expect("validated supported name");
                call.validate_signature(&["*mut lv_obj_t"], Some("*mut lv_obj_t"))?;
            }
            if let AdapterPolicy::Custom(handler) = rule.policy
                && handler.trim().is_empty()
            {
                return Err(GeneratorError::new(format!(
                    "empty custom handler for {}",
                    rule.name
                )));
            }
        }

        Ok(Self { calls, adapters })
    }

    pub fn calls(&self) -> &[&'a Call] {
        &self.calls
    }

    pub fn adapter_for(&self, name: &str) -> Option<AdapterPolicy> {
        self.adapters.get(name).copied()
    }

    /// Deterministic build artifact inventory. It is emitted to OUT_DIR only;
    /// no registry or metadata table is linked into the runtime.
    pub fn inventory_json(&self, catalog: &CallCatalog) -> String {
        fn escape(value: &str) -> String {
            let mut escaped = String::with_capacity(value.len());
            for character in value.chars() {
                match character {
                    '"' => escaped.push_str("\\\""),
                    '\\' => escaped.push_str("\\\\"),
                    '\n' => escaped.push_str("\\n"),
                    '\r' => escaped.push_str("\\r"),
                    '\t' => escaped.push_str("\\t"),
                    character if character.is_control() => {
                        use core::fmt::Write;
                        let _ = write!(escaped, "\\u{:04x}", character as u32);
                    }
                    character => escaped.push(character),
                }
            }
            escaped
        }

        let mut json = String::from("{\"calls\":[");
        for (index, call) in catalog.calls().iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            let signature = call.signature.to_token_stream().to_string();
            let return_type = match &call.signature.output {
                ReturnType::Default => "void".to_string(),
                ReturnType::Type(_, output) => output.to_token_stream().to_string(),
            };
            let (guest, adapter, handler, reason, ownership) = match self.adapter_for(&call.name) {
                Some(AdapterPolicy::Manual) => (true, "manual", "null", "null", "handwritten"),
                Some(AdapterPolicy::AutoObjectScalar(_)) => (
                    true,
                    "auto_object_scalar",
                    "null",
                    "null",
                    "scalar_no_retention",
                ),
                Some(AdapterPolicy::Custom(handler)) => {
                    (true, "custom", handler, "null", "handwritten")
                }
                Some(AdapterPolicy::Object { .. }) => {
                    (true, "object", "null", "null", "scalar_no_retention")
                }
                Some(AdapterPolicy::Create(_)) => {
                    (true, "create", "null", "null", "host_object_creation")
                }
                None => (
                    false,
                    "null",
                    "null",
                    "\"No host adapter registered\"",
                    "unknown_or_not_supported",
                ),
            };
            use core::fmt::Write;
            let _ = write!(
                json,
                "{{\"name\":\"{}\",\"id\":{},\"argument_count\":{},\"return\":\"{}\",\"signature\":\"{}\",\"discovered\":true,\"guest_wrapper\":{},\"host_adapter\":{},\"custom_handler\":{},\"ownership\":\"{}\",\"unsupported_reason\":{}}}",
                escape(&call.name),
                call.id,
                call.argument_count,
                escape(&return_type),
                escape(&signature),
                guest,
                if adapter == "null" {
                    "null".to_string()
                } else {
                    format!("\"{adapter}\"")
                },
                if handler == "null" {
                    "null".to_string()
                } else {
                    format!("\"{}\"", escape(handler))
                },
                ownership,
                reason,
            );
        }
        json.push_str("]}\n");
        json
    }
}

fn validate_auto_object_scalar(
    call: &Call,
    adapter: AutoObjectScalar,
) -> Result<(), GeneratorError> {
    if !call.generic_safe() {
        return Err(GeneratorError::new(format!(
            "automatic adapter for {} uses a non-scalar or pointer-retaining signature",
            call.name
        )));
    }
    let (arguments, output): (&[&str], Option<&str>) = match adapter {
        AutoObjectScalar::SetI32 => (&["*mut lv_obj_t", "i32"], None),
        AutoObjectScalar::SetPairI32 => (&["*mut lv_obj_t", "i32", "i32"], None),
        AutoObjectScalar::GetI32 => (&["*const lv_obj_t"], Some("i32")),
    };
    call.validate_signature(arguments, output)
}
