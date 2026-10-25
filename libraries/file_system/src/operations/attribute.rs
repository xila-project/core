use crate::{Attributes, Result};

pub trait AttributeOperations {
    /// Populate the requested attributes for the open file or directory.
    ///
    /// On entry, `attributes.get_mask()` is the request mask. On success, the
    /// mask contains only requested fields that were populated with meaningful
    /// values. Fields unavailable for this object or backend are omitted. An
    /// empty mask succeeds without reading any fields.
    fn get_attributes(
        &self,
        context: &mut crate::Context,
        attributes: &mut Attributes,
    ) -> Result<()>;

    /// Change the fields selected by `attributes.get_mask()`.
    ///
    /// An empty mask succeeds without changing any fields. If any requested
    /// field is unsupported, return [`crate::Error::UnsupportedOperation`]
    /// before changing a supported field. A later backend I/O or permission
    /// failure does not guarantee rollback of earlier changes.
    fn set_attributes(&self, context: &mut crate::Context, attributes: &Attributes) -> Result<()>;
}
