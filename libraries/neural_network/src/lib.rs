#![no_std]

extern crate alloc;

mod backend;
mod command;
mod device;

pub use backend::{Backend, NeuralNetworkDevice};
pub use command::*;
