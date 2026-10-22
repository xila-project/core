#[cfg(target_pointer_width = "32")]
pub type EntryIdentifierInner = u16;
#[cfg(target_pointer_width = "64")]
pub type EntryIdentifierInner = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct EntryIdentifier(EntryIdentifierInner);

impl EntryIdentifier {
    pub const fn new(identifier: EntryIdentifierInner) -> Self {
        Self(identifier)
    }
}

impl From<EntryIdentifierInner> for EntryIdentifier {
    fn from(internal_directory_entry_identifier: EntryIdentifierInner) -> Self {
        EntryIdentifier(internal_directory_entry_identifier)
    }
}

impl From<EntryIdentifier> for EntryIdentifierInner {
    fn from(internal_directory_entry_identifier: EntryIdentifier) -> Self {
        internal_directory_entry_identifier.0
    }
}
