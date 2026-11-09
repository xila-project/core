use crate::{
    BaseOperations, ControlCommand, DirectBaseOperations, MountOperations, define_command,
};

// Kind byte `b'T'` is registered as `device::kind::TERMINAL` (kept as a literal here because `device` depends on `file_system`).
define_command!(IS_A_TERMINAL, Read, b'T', 0, (), bool);

pub trait CharacterDevice: BaseOperations + MountOperations {}

pub trait DirectCharacterDevice: DirectBaseOperations + MountOperations {}

impl<T: DirectCharacterDevice> CharacterDevice for T {}
