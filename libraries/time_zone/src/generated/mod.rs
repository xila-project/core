//! Generated IANA tables. Do not edit by hand.
//!
//! Run `tools/generate_iana_table.py` to regenerate `all.rs` and `europe.rs`
//! from a compiled IANA time zone database, then run `cargo fmt`.

#[cfg(feature = "iana-all")]
pub(crate) mod all;
#[cfg(all(feature = "iana-europe", not(feature = "iana-all")))]
pub(crate) mod europe;
