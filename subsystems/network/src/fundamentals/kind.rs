#[repr(u8)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceKind {
    #[default]
    Unknown,
    Ethernet,
    WiFi,
}
