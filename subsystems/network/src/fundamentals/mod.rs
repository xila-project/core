mod cidr;
mod dns;
mod duration;
mod endpoint;
mod ip;
mod kind;
mod port;
mod route;
mod udp;

pub use cidr::*;
pub use dns::*;
pub use duration::*;
pub use endpoint::*;
pub use ip::*;
pub use kind::*;
pub use port::*;
pub use route::*;
pub use udp::*;

pub type MacAddress = [u8; 6];
