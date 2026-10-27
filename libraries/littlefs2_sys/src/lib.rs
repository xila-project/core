#![no_std]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

// We need this extern crate statement to indicate that we want to link against tinyrlibc.
#[cfg(feature = "tinyrlibc")]
extern crate tinyrlibc;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
