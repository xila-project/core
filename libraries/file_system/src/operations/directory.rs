use crate::{AttributeOperations, Entry, Result, Size};

pub trait DirectoryOperations: AttributeOperations {
    fn read(&self, context: &mut crate::Context) -> Result<Option<Entry>>;

    fn set_position(&self, context: &mut crate::Context, position: Size) -> Result<()>;

    fn get_position(&self, context: &mut crate::Context) -> Result<Size>;

    fn rewind(&self, context: &mut crate::Context) -> Result<()>;

    fn close(&self, context: &mut crate::Context) -> Result<()>;
}
