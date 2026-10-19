use crate::{
    BaseOperations, ControlCommand, DirectBaseOperations, MountOperations, define_command,
};

define_command!(IS_A_TERMINAL, Read, b'T', 0, (), bool);

pub trait CharacterDevice: BaseOperations + MountOperations {}

pub trait DirectCharacterDevice: DirectBaseOperations + MountOperations {}

impl<T: DirectCharacterDevice> CharacterDevice for T {}
