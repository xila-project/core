use alloc::string::String;
use file_system::{Attributes, Entry, IoErrorKind, Kind, Result, Size};

use crate::file_system::State;
use crate::node::ROOT_NODE_INDEX;

/// Open directory cursor.
///
/// Positions:
/// - `0`: `.` (the directory itself)
/// - `1`: `..` (the parent directory, or root for `/`)
/// - `>= 2`: scan child entries starting at node slot index `position - 2`
pub(crate) struct Directory {
    node_index: usize,
    generation: u32,
    position: Size,
}

impl Directory {
    pub(crate) const fn new(node_index: usize, generation: u32) -> Self {
        Self {
            node_index,
            generation,
            position: 0,
        }
    }

    fn verify_open<'a>(&self, state: &'a State) -> Result<&'a crate::node::Node> {
        let node = state.get_node_with_generation(self.node_index, self.generation)?;
        if !node.is_directory() {
            return Err(IoErrorKind::NotADirectory);
        }
        Ok(node)
    }

    fn verify_open_mutable<'a>(&self, state: &'a mut State) -> Result<&'a mut crate::node::Node> {
        let node = state.get_node_with_generation_mutable(self.node_index, self.generation)?;
        if !node.is_directory() {
            return Err(IoErrorKind::NotADirectory);
        }
        Ok(node)
    }

    pub(crate) fn read(&mut self, state: &State) -> Result<Option<Entry>> {
        let directory_node = self.verify_open(state)?;

        if self.position == 0 {
            self.position = 1;
            return Ok(Some(Entry::new(
                directory_node.inode,
                String::from("."),
                Kind::Directory,
                0,
                directory_node.modification,
            )));
        }

        if self.position == 1 {
            let parent_node = state.get_node(directory_node.parent as usize)?;
            self.position = 2;
            return Ok(Some(Entry::new(
                parent_node.inode,
                String::from(".."),
                Kind::Directory,
                0,
                parent_node.modification,
            )));
        }

        let start_slot = ((self.position - 2) as usize).max(ROOT_NODE_INDEX + 1);
        let parent_index = self.node_index as u32;

        if let Some((slot_index, child)) = state.next_child(parent_index, start_slot) {
            self.position = (slot_index + 3) as Size;
            return Ok(Some(Entry::new(
                child.inode,
                String::from(child.name.as_ref()),
                child.kind,
                0,
                child.modification,
            )));
        }

        self.position = (state.slot_count() + 2) as Size;
        Ok(None)
    }

    pub(crate) fn get_position(&self, state: &State) -> Result<Size> {
        self.verify_open(state)?;
        Ok(self.position)
    }

    pub(crate) fn set_position(&mut self, state: &State, position: Size) -> Result<()> {
        self.verify_open(state)?;
        self.position = position;
        Ok(())
    }

    pub(crate) fn rewind(&mut self, state: &State) -> Result<()> {
        self.verify_open(state)?;
        self.position = 0;
        Ok(())
    }

    pub(crate) fn get_attributes(&self, state: &State, attributes: &mut Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }

        let node = self.verify_open(state)?;
        node.to_attributes().read_into(attributes);
        Ok(())
    }

    pub(crate) fn set_attributes(&self, state: &mut State, attributes: &Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }

        let node = self.verify_open_mutable(state)?;
        let mut node_attributes = node.to_attributes();
        node_attributes.apply_from(attributes)?;
        node.set_attributes(&node_attributes);
        Ok(())
    }
}
