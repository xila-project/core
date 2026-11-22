use alloc::{boxed::Box, vec, vec::Vec};
use core::task::{Context as TaskContext, Poll};
use file_system::{
    AttributeFlags, AttributeOperations, Attributes, BaseOperations, Context, DirectoryOperations,
    FileOperations, FileSystemOperations, Flags, IoErrorKind, Kind, MountOperations, Path,
    Permissions, Position, Result, Size, mount::MutexMountWrapper,
};
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

use crate::{
    attributes::{NodeAttributes, SUPPORTED_ATTRIBUTE_FLAGS},
    directory::Directory,
    node::{Node, ROOT_NODE_INDEX},
};

type SharedContext<T> = Arc<Mutex<CriticalSectionRawMutex, Box<T>>>;

enum TargetPath<'a> {
    Root,
    ExistingComponent(usize),
    Child {
        parent_index: usize,
        name: &'a str,
        trailing_slash: bool,
    },
}

pub(crate) struct State {
    nodes: Vec<Option<Node>>,
    next_generation: u32,
}

impl State {
    pub(crate) fn new() -> Self {
        let root = Node::new(
            Box::from(""),
            ROOT_NODE_INDEX as u32,
            1,
            Kind::Directory,
            Permissions::DIRECTORY_DEFAULT,
        );
        Self {
            nodes: vec![Some(root)],
            next_generation: 2,
        }
    }

    pub(crate) fn slot_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn get_node(&self, index: usize) -> Result<&Node> {
        self.nodes
            .get(index)
            .and_then(|slot| slot.as_ref())
            .ok_or(IoErrorKind::NotFound)
    }

    pub(crate) fn get_node_mutable(&mut self, index: usize) -> Result<&mut Node> {
        self.nodes
            .get_mut(index)
            .and_then(|slot| slot.as_mut())
            .ok_or(IoErrorKind::NotFound)
    }

    pub(crate) fn get_node_with_generation(&self, index: usize, generation: u32) -> Result<&Node> {
        let node = self.get_node(index)?;
        if node.generation != generation {
            return Err(IoErrorKind::NotFound);
        }
        Ok(node)
    }

    pub(crate) fn get_node_with_generation_mutable(
        &mut self,
        index: usize,
        generation: u32,
    ) -> Result<&mut Node> {
        let node = self.get_node_mutable(index)?;
        if node.generation != generation {
            return Err(IoErrorKind::NotFound);
        }
        Ok(node)
    }

    pub(crate) fn next_child(
        &self,
        parent_index: u32,
        start_slot: usize,
    ) -> Option<(usize, &Node)> {
        let start = start_slot.max(ROOT_NODE_INDEX + 1);
        self.nodes
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(slot_index, slot)| {
                slot.as_ref()
                    .filter(|node| node.parent == parent_index)
                    .map(|node| (slot_index, node))
            })
    }

    fn find_child(&self, parent_index: u32, name: &str) -> Option<usize> {
        self.nodes
            .iter()
            .enumerate()
            .skip(ROOT_NODE_INDEX + 1)
            .find_map(|(slot_index, slot)| {
                slot.as_ref()
                    .filter(|node| node.parent == parent_index && node.name.as_ref() == name)
                    .map(|_| slot_index)
            })
    }

    fn has_children(&self, parent_index: u32) -> bool {
        self.next_child(parent_index, ROOT_NODE_INDEX + 1).is_some()
    }

    fn is_ancestor(&self, candidate_ancestor: usize, start_index: usize) -> bool {
        let mut current = start_index;
        loop {
            if current == candidate_ancestor {
                return true;
            }
            if current == ROOT_NODE_INDEX {
                return false;
            }
            match self.nodes.get(current).and_then(|slot| slot.as_ref()) {
                Some(node) => current = node.parent as usize,
                None => return false,
            }
        }
    }

    fn allocate_generation(&mut self) -> u32 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        generation
    }

    fn insert_node(&mut self, parent_index: usize, name: &str, kind: Kind) -> Result<usize> {
        let generation = self.allocate_generation();
        let permissions = if kind == Kind::Directory {
            Permissions::DIRECTORY_DEFAULT
        } else {
            Permissions::NONE
        };
        let node = Node::new(
            Box::from(name),
            parent_index as u32,
            generation,
            kind,
            permissions,
        );

        if let Some(free_index) = self
            .nodes
            .iter()
            .enumerate()
            .skip(ROOT_NODE_INDEX + 1)
            .find_map(|(index, slot)| slot.is_none().then_some(index))
        {
            self.nodes[free_index] = Some(node);
            return Ok(free_index);
        }

        if self.nodes.len() >= u32::MAX as usize {
            return Err(IoErrorKind::NoSpace);
        }

        let new_index = self.nodes.len();
        self.nodes.push(Some(node));
        Ok(new_index)
    }

    fn remove_slot(&mut self, index: usize) {
        if index > ROOT_NODE_INDEX && index < self.nodes.len() {
            self.nodes[index] = None;
            while self.nodes.len() > ROOT_NODE_INDEX + 1
                && self.nodes.last().is_some_and(Option::is_none)
            {
                self.nodes.pop();
            }
        }
    }

    fn validate_path_string(path: &Path) -> Result<&str> {
        // The virtual file system addresses a mount point's own directory with
        // an empty relative path: like LittleFS, resolve it to the root.
        let path_string = path.as_str();
        if path_string.contains('\0') {
            return Err(IoErrorKind::InvalidArgument);
        }
        Ok(path_string)
    }

    fn resolve_segments(&self, path_string: &str) -> Result<usize> {
        let mut current = ROOT_NODE_INDEX;

        for segment in path_string.split('/') {
            if segment.is_empty() {
                continue;
            }
            if !self.get_node(current)?.is_directory() {
                return Err(IoErrorKind::NotADirectory);
            }
            match segment {
                "." => {}
                ".." => {
                    current = self.get_node(current)?.parent as usize;
                }
                name => {
                    current = self
                        .find_child(current as u32, name)
                        .ok_or(IoErrorKind::NotFound)?;
                }
            }
        }

        Ok(current)
    }

    fn resolve_existing_path(&self, path: &Path) -> Result<usize> {
        let path_string = Self::validate_path_string(path)?;
        let trailing_slash = path_string.len() > 1 && path_string.ends_with('/');
        let node_index = self.resolve_segments(path_string)?;

        if trailing_slash && !self.get_node(node_index)?.is_directory() {
            return Err(IoErrorKind::NotADirectory);
        }

        Ok(node_index)
    }

    fn resolve_target_path<'a>(&self, path: &'a Path) -> Result<TargetPath<'a>> {
        let path_string = Self::validate_path_string(path)?;
        let trailing_slash = path_string.len() > 1 && path_string.ends_with('/');
        let trimmed = path_string.trim_end_matches('/');

        if trimmed.is_empty() {
            return Ok(TargetPath::Root);
        }

        let (parent_string, name) = match trimmed.rfind('/') {
            Some(separator_index) => (&trimmed[..separator_index], &trimmed[separator_index + 1..]),
            None => ("", trimmed),
        };

        if name == "." || name == ".." {
            let existing = self.resolve_segments(trimmed)?;
            if trailing_slash && !self.get_node(existing)?.is_directory() {
                return Err(IoErrorKind::NotADirectory);
            }
            return Ok(TargetPath::ExistingComponent(existing));
        }

        let parent_index = if parent_string.is_empty() {
            ROOT_NODE_INDEX
        } else {
            let index = self.resolve_segments(parent_string)?;
            if !self.get_node(index)?.is_directory() {
                return Err(IoErrorKind::NotADirectory);
            }
            index
        };

        Ok(TargetPath::Child {
            parent_index,
            name,
            trailing_slash,
        })
    }

    fn create_directory(&mut self, path: &Path) -> Result<()> {
        match self.resolve_target_path(path)? {
            TargetPath::Root | TargetPath::ExistingComponent(_) => Err(IoErrorKind::AlreadyExists),
            TargetPath::Child {
                parent_index,
                name,
                trailing_slash: _,
            } => {
                if self.find_child(parent_index as u32, name).is_some() {
                    return Err(IoErrorKind::AlreadyExists);
                }
                self.insert_node(parent_index, name, Kind::Directory)?;
                Ok(())
            }
        }
    }

    fn create_file(&mut self, path: &Path) -> Result<()> {
        match self.resolve_target_path(path)? {
            TargetPath::Root => Err(IoErrorKind::IsADirectory),
            TargetPath::ExistingComponent(existing_index) => {
                if self.get_node(existing_index)?.is_directory() {
                    Err(IoErrorKind::IsADirectory)
                } else {
                    Err(IoErrorKind::AlreadyExists)
                }
            }
            TargetPath::Child {
                parent_index,
                name,
                trailing_slash,
            } => {
                if let Some(existing_index) = self.find_child(parent_index as u32, name) {
                    if self.get_node(existing_index)?.is_directory() {
                        return Err(IoErrorKind::IsADirectory);
                    }
                    return Err(IoErrorKind::AlreadyExists);
                }
                if trailing_slash {
                    return Err(IoErrorKind::InvalidPath);
                }
                self.insert_node(parent_index, name, Kind::File)?;
                Ok(())
            }
        }
    }

    fn remove(&mut self, path: &Path) -> Result<()> {
        let node_index = self.resolve_existing_path(path)?;
        if node_index == ROOT_NODE_INDEX {
            return Err(IoErrorKind::InvalidArgument);
        }

        if self.get_node(node_index)?.is_directory() && self.has_children(node_index as u32) {
            return Err(IoErrorKind::DirectoryNotEmpty);
        }

        self.remove_slot(node_index);
        Ok(())
    }

    fn rename(&mut self, source: &Path, destination: &Path) -> Result<()> {
        let source_index = self.resolve_existing_path(source)?;
        if source_index == ROOT_NODE_INDEX {
            return Err(IoErrorKind::InvalidArgument);
        }

        let source_is_directory = self.get_node(source_index)?.is_directory();

        match self.resolve_target_path(destination)? {
            TargetPath::Root => Err(IoErrorKind::InvalidArgument),
            TargetPath::ExistingComponent(existing_index) => {
                if existing_index == source_index {
                    Ok(())
                } else {
                    Err(IoErrorKind::InvalidArgument)
                }
            }
            TargetPath::Child {
                parent_index,
                name,
                trailing_slash,
            } => {
                if source_is_directory && self.is_ancestor(source_index, parent_index) {
                    return Err(IoErrorKind::InvalidArgument);
                }

                if let Some(destination_index) = self.find_child(parent_index as u32, name) {
                    if destination_index == source_index {
                        return Ok(());
                    }

                    let destination_is_directory = self.get_node(destination_index)?.is_directory();
                    if source_is_directory && !destination_is_directory {
                        return Err(IoErrorKind::NotADirectory);
                    }
                    if !source_is_directory && destination_is_directory {
                        return Err(IoErrorKind::IsADirectory);
                    }
                    if destination_is_directory && self.has_children(destination_index as u32) {
                        return Err(IoErrorKind::DirectoryNotEmpty);
                    }
                    if trailing_slash && !source_is_directory {
                        return Err(IoErrorKind::NotADirectory);
                    }

                    self.remove_slot(destination_index);
                } else if trailing_slash && !source_is_directory {
                    return Err(IoErrorKind::NotADirectory);
                }

                let source_node = self.get_node_mutable(source_index)?;
                source_node.parent = parent_index as u32;
                if source_node.name.as_ref() != name {
                    source_node.name = Box::from(name);
                }
                Ok(())
            }
        }
    }

    fn get_attributes(&self, path: &Path, attributes: &mut Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }

        let node_index = self.resolve_existing_path(path)?;
        let node = self.get_node(node_index)?;
        node.to_attributes().read_into(attributes);
        Ok(())
    }

    fn set_attributes(&mut self, path: &Path, attributes: &Attributes) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }

        let node_index = self.resolve_existing_path(path)?;
        let node = self.get_node_mutable(node_index)?;
        let mut node_attributes: NodeAttributes = node.to_attributes();
        node_attributes.apply_from(attributes)?;
        node.set_attributes(&node_attributes);
        Ok(())
    }
}

/// Thread-safe in-RAM file system holding directories and metadata-only device/pipe nodes.
pub struct FileSystem {
    state: MutexMountWrapper<CriticalSectionRawMutex, State>,
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem {
    /// Create a new mounted in-memory file system containing an empty root directory.
    pub fn new() -> Self {
        Self {
            state: MutexMountWrapper::new_mounted(State::new()),
        }
    }

    fn with_state<T>(&self, operation: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let mut state = self.state.try_get()?;
        operation(&mut state)
    }

    fn with_directory<T>(
        &self,
        context: &mut Context,
        operation: impl FnOnce(&mut State, &mut Directory) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.state.try_get()?;
        let directory = context
            .get_private_data_of_type::<SharedContext<Directory>>()
            .ok_or(IoErrorKind::InvalidArgument)?;
        let mut directory = directory.try_lock().map_err(|_| IoErrorKind::WouldBlock)?;
        operation(&mut state, &mut directory)
    }
}

impl MountOperations for FileSystem {
    fn mount(&self) -> Result<()> {
        if self.state.is_mounted()? {
            return Ok(());
        }
        self.state.mount(State::new())
    }

    fn unmount(&self) -> Result<()> {
        self.state.unmount()
    }
}

impl FileSystemOperations for FileSystem {
    fn supports_confined_namespace(&self) -> bool {
        true
    }

    fn writable_attributes(&self) -> AttributeFlags {
        SUPPORTED_ATTRIBUTE_FLAGS
    }

    fn persistent_attributes(&self) -> AttributeFlags {
        AttributeFlags::empty()
    }

    fn lookup_directory(&self, context: &mut Context, path: &Path) -> Result<()> {
        self.with_state(|state| {
            let node_index = state.resolve_existing_path(path)?;
            let node = state.get_node(node_index)?;
            if !node.is_directory() {
                return Err(IoErrorKind::NotADirectory);
            }

            let directory = Box::new(Directory::new(node_index, node.generation));
            context.set_private_data(Box::new(Arc::new(
                Mutex::<CriticalSectionRawMutex, _>::new(directory),
            )));
            Ok(())
        })
    }

    fn lookup_file(&self, _context: &mut Context, path: &Path, _flags: Flags) -> Result<()> {
        self.with_state(|state| {
            let node_index = state.resolve_existing_path(path)?;
            if state.get_node(node_index)?.is_directory() {
                return Err(IoErrorKind::IsADirectory);
            }
            Err(IoErrorKind::Unsupported)
        })
    }

    fn create_directory(&self, path: &Path) -> Result<()> {
        self.with_state(|state| state.create_directory(path))
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.with_state(|state| state.create_file(path))
    }

    fn remove(&self, path: &Path) -> Result<()> {
        self.with_state(|state| state.remove(path))
    }

    fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
        self.with_state(|state| state.rename(source, destination))
    }

    fn get_attributes(&self, path: &Path, attributes: &mut Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        self.with_state(|state| state.get_attributes(path, attributes))
    }

    fn set_attributes(&self, path: &Path, attributes: &Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        self.with_state(|state| state.set_attributes(path, attributes))
    }
}

impl AttributeOperations for FileSystem {
    fn get_attributes(&self, context: &mut Context, attributes: &mut Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        self.with_directory(context, |state, directory| {
            directory.get_attributes(state, attributes)
        })
    }

    fn set_attributes(&self, context: &mut Context, attributes: &Attributes) -> Result<()> {
        if attributes.get_mask().is_empty() {
            return Ok(());
        }
        self.with_directory(context, |state, directory| {
            directory.set_attributes(state, attributes)
        })
    }
}

impl BaseOperations for FileSystem {
    fn read(
        &self,
        _context: &mut Context,
        _task_context: &mut TaskContext<'_>,
        _buffer: &mut [u8],
        _absolute_position: Size,
    ) -> Poll<Result<usize>> {
        Poll::Ready(Err(IoErrorKind::Unsupported))
    }

    fn write(
        &self,
        _context: &mut Context,
        _task_context: &mut TaskContext<'_>,
        _buffer: &[u8],
        _absolute_position: Size,
    ) -> Poll<Result<usize>> {
        Poll::Ready(Err(IoErrorKind::Unsupported))
    }

    fn set_position(
        &self,
        _context: &mut Context,
        _current: Size,
        _position: &Position,
    ) -> Result<Size> {
        Err(IoErrorKind::Unsupported)
    }

    fn flush(
        &self,
        _context: &mut Context,
        _task_context: &mut TaskContext<'_>,
    ) -> Poll<Result<()>> {
        Poll::Ready(Err(IoErrorKind::Unsupported))
    }

    fn open(&self, _context: &mut Context) -> Result<()> {
        Err(IoErrorKind::Unsupported)
    }

    fn close(&self, _context: &mut Context) -> Result<()> {
        Err(IoErrorKind::Unsupported)
    }

    fn clone_context(&self, context: &Context) -> Result<Context> {
        if let Some(directory) = context.get_private_data_of_type::<SharedContext<Directory>>() {
            Ok(Context::new(Some(directory.clone())))
        } else {
            Err(IoErrorKind::InvalidArgument)
        }
    }
}

impl FileOperations for FileSystem {}

impl DirectoryOperations for FileSystem {
    fn read(&self, context: &mut Context) -> Result<Option<file_system::Entry>> {
        self.with_directory(context, |state, directory| directory.read(state))
    }

    fn get_position(&self, context: &mut Context) -> Result<Size> {
        self.with_directory(context, |state, directory| directory.get_position(state))
    }

    fn set_position(&self, context: &mut Context, position: Size) -> Result<()> {
        self.with_directory(context, |state, directory| {
            directory.set_position(state, position)
        })
    }

    fn rewind(&self, context: &mut Context) -> Result<()> {
        self.with_directory(context, |state, directory| directory.rewind(state))
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        let _state = self.state.try_get()?;
        if context
            .take_private_data_of_type::<SharedContext<Directory>>()
            .is_none()
        {
            return Err(IoErrorKind::InvalidArgument);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::String, vec::Vec};
    use file_system::{
        AccessFlags,
        file_system::tests::{
            FileSystemTestCapabilities, test_attribute_operations_with_capabilities,
            test_directory_attributes, test_directory_entry_metadata, test_directory_operations,
            test_directory_position_operations, test_directory_read_operations,
            test_directory_rewind, test_empty_directory_read, test_error_handling,
            test_file_creation_edge_cases, test_file_system_consistency, test_invalid_paths,
            test_long_filenames, test_nested_directories, test_rename_edge_cases,
            test_root_directory_operations, test_special_characters_in_names,
        },
    };
    use users::{GroupIdentifier, UserIdentifier};

    fn test_attribute_capabilities() -> FileSystemTestCapabilities {
        FileSystemTestCapabilities {
            readable: SUPPORTED_ATTRIBUTE_FLAGS | AttributeFlags::Size,
            writable: SUPPORTED_ATTRIBUTE_FLAGS,
        }
    }

    #[test]
    fn shared_conformance_directory_operations() {
        let file_system = FileSystem::new();
        test_directory_operations(&file_system);
    }

    #[test]
    fn shared_conformance_nested_directories() {
        let file_system = FileSystem::new();
        test_nested_directories(&file_system);
    }

    #[test]
    fn shared_conformance_file_creation_edge_cases() {
        let file_system = FileSystem::new();
        test_file_creation_edge_cases(&file_system);
    }

    #[test]
    fn shared_conformance_rename_edge_cases() {
        let file_system = FileSystem::new();
        test_rename_edge_cases(&file_system);
    }

    #[test]
    fn shared_conformance_attribute_operations() {
        let file_system = FileSystem::new();
        test_attribute_operations_with_capabilities(&file_system, test_attribute_capabilities());
    }

    #[test]
    fn shared_conformance_directory_attributes() {
        let file_system = FileSystem::new();
        test_directory_attributes(&file_system);
    }

    #[test]
    fn shared_conformance_error_handling() {
        let file_system = FileSystem::new();
        test_error_handling(&file_system);
    }

    #[test]
    fn shared_conformance_invalid_paths() {
        let file_system = FileSystem::new();
        test_invalid_paths(&file_system);
    }

    #[test]
    fn shared_conformance_file_system_consistency() {
        let file_system = FileSystem::new();
        test_file_system_consistency(&file_system);
    }

    #[test]
    fn shared_conformance_directory_read_operations() {
        let file_system = FileSystem::new();
        test_directory_read_operations(&file_system);
    }

    #[test]
    fn shared_conformance_directory_position_operations() {
        let file_system = FileSystem::new();
        test_directory_position_operations(&file_system);
    }

    #[test]
    fn shared_conformance_directory_rewind() {
        let file_system = FileSystem::new();
        test_directory_rewind(&file_system);
    }

    #[test]
    fn shared_conformance_empty_directory_read() {
        let file_system = FileSystem::new();
        test_empty_directory_read(&file_system);
    }

    #[test]
    fn shared_conformance_directory_entry_metadata() {
        let file_system = FileSystem::new();
        test_directory_entry_metadata(&file_system);
    }

    #[test]
    fn shared_conformance_root_directory_operations() {
        let file_system = FileSystem::new();
        test_root_directory_operations(&file_system);
    }

    #[test]
    fn shared_conformance_long_filenames() {
        let file_system = FileSystem::new();
        test_long_filenames(&file_system);
    }

    #[test]
    fn shared_conformance_special_characters_in_names() {
        let file_system = FileSystem::new();
        test_special_characters_in_names(&file_system);
    }

    #[test]
    fn node_memory_footprint_is_compact() {
        #[cfg(target_pointer_width = "32")]
        assert_eq!(core::mem::size_of::<Node>(), 64);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(core::mem::size_of::<Node>(), 72);

        assert_eq!(
            core::mem::size_of::<Option<Node>>(),
            core::mem::size_of::<Node>()
        );
    }

    #[test]
    fn lookup_file_and_content_operations_return_unsupported_operation() {
        let file_system = FileSystem::new();
        let node_path = Path::new("/serial0");
        file_system.create_file(node_path).unwrap();

        let mut context = Context::new_empty();
        assert_eq!(
            file_system.lookup_file(&mut context, node_path, AccessFlags::Read.into()),
            Err(IoErrorKind::Unsupported)
        );
        assert_eq!(
            file_system.lookup_file(&mut context, Path::ROOT, AccessFlags::Read.into()),
            Err(IoErrorKind::IsADirectory)
        );
        assert_eq!(
            file_system.lookup_file(
                &mut context,
                Path::new("/missing"),
                AccessFlags::Read.into()
            ),
            Err(IoErrorKind::NotFound)
        );

        let mut buffer = [0u8; 8];
        assert_eq!(
            BaseOperations::read_sync(&file_system, &mut context, &mut buffer, 0),
            Err(IoErrorKind::Unsupported)
        );
        assert_eq!(
            BaseOperations::write_sync(&file_system, &mut context, b"data", 0),
            Err(IoErrorKind::Unsupported)
        );
        assert_eq!(
            BaseOperations::flush_sync(&file_system, &mut context),
            Err(IoErrorKind::Unsupported)
        );
        assert_eq!(
            BaseOperations::set_position(&file_system, &mut context, 0, &Position::Start(0)),
            Err(IoErrorKind::Unsupported)
        );
        assert_eq!(
            BaseOperations::open(&file_system, &mut context),
            Err(IoErrorKind::Unsupported)
        );
        assert_eq!(
            BaseOperations::close(&file_system, &mut context),
            Err(IoErrorKind::Unsupported)
        );
    }

    #[test]
    fn device_and_pipe_attributes_round_trip_and_links_remain_unsupported() {
        let file_system = FileSystem::new();
        assert_eq!(file_system.writable_attributes(), SUPPORTED_ATTRIBUTE_FLAGS);
        assert_eq!(file_system.persistent_attributes(), AttributeFlags::empty());

        let node_path = Path::new("/tty0");
        file_system.create_file(node_path).unwrap();

        let updated = Attributes::new()
            .set_kind(Kind::CharacterDevice)
            .set_inode(7)
            .set_user(UserIdentifier::new(1000))
            .set_group(GroupIdentifier::new(100))
            .set_permissions(Permissions::DEVICE_DEFAULT);
        FileSystemOperations::set_attributes(&file_system, node_path, &updated).unwrap();

        let mut actual = Attributes::new().set_mask(AttributeFlags::all());
        FileSystemOperations::get_attributes(&file_system, node_path, &mut actual).unwrap();
        assert_eq!(actual.get_mask(), SUPPORTED_ATTRIBUTE_FLAGS);
        assert_eq!(actual.get_kind(), Some(&Kind::CharacterDevice));
        assert_eq!(actual.get_inode(), Some(&7));
        assert_eq!(actual.get_user(), Some(&UserIdentifier::new(1000)));
        assert_eq!(actual.get_group(), Some(&GroupIdentifier::new(100)));
        assert_eq!(actual.get_permissions(), Some(&Permissions::DEVICE_DEFAULT));
        assert_eq!(actual.get_size(), None);
        assert_eq!(actual.get_links(), None);

        assert_eq!(
            file_system.create_symlink(Path::new("/tty0"), Path::new("/tty_link")),
            Err(IoErrorKind::Unsupported)
        );
        assert_eq!(
            file_system.link(Path::new("/tty0"), Path::new("/tty_hardlink")),
            Err(IoErrorKind::Unsupported)
        );
        let mut symlink_buffer = [0u8; 16];
        assert_eq!(
            file_system.read_symlink(Path::new("/tty_link"), &mut symlink_buffer),
            Err(IoErrorKind::Unsupported)
        );
    }

    #[test]
    fn rename_and_move_semantics_match_little_fs() {
        let file_system = FileSystem::new();
        file_system.create_directory(Path::new("/uart")).unwrap();
        file_system.create_directory(Path::new("/serial")).unwrap();
        file_system.create_file(Path::new("/uart/uart0")).unwrap();
        file_system.create_file(Path::new("/uart/uart1")).unwrap();

        let uart0_attributes = Attributes::new()
            .set_kind(Kind::CharacterDevice)
            .set_inode(11);
        FileSystemOperations::set_attributes(
            &file_system,
            Path::new("/uart/uart0"),
            &uart0_attributes,
        )
        .unwrap();

        // Rename within the same directory.
        file_system
            .rename(Path::new("/uart/uart0"), Path::new("/uart/console"))
            .unwrap();

        let mut read_back =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);
        assert_eq!(
            FileSystemOperations::get_attributes(
                &file_system,
                Path::new("/uart/uart0"),
                &mut read_back
            ),
            Err(IoErrorKind::NotFound)
        );
        FileSystemOperations::get_attributes(
            &file_system,
            Path::new("/uart/console"),
            &mut read_back,
        )
        .unwrap();
        assert_eq!(read_back.get_kind(), Some(&Kind::CharacterDevice));
        assert_eq!(read_back.get_inode(), Some(&11));

        // Move across directories and overwrite existing target file.
        file_system
            .create_file(Path::new("/serial/console"))
            .unwrap();
        file_system
            .rename(Path::new("/uart/console"), Path::new("/serial/console"))
            .unwrap();
        FileSystemOperations::get_attributes(
            &file_system,
            Path::new("/serial/console"),
            &mut read_back,
        )
        .unwrap();
        assert_eq!(read_back.get_inode(), Some(&11));

        // Moving a directory beneath itself is rejected.
        file_system
            .create_directory(Path::new("/uart/child"))
            .unwrap();
        assert_eq!(
            file_system.rename(Path::new("/uart"), Path::new("/uart/child/nested")),
            Err(IoErrorKind::InvalidArgument)
        );

        // Type mismatch and non-empty directory overwrite errors.
        assert_eq!(
            file_system.rename(Path::new("/serial/console"), Path::new("/uart")),
            Err(IoErrorKind::IsADirectory)
        );
        assert_eq!(
            file_system.rename(Path::new("/uart"), Path::new("/serial/console")),
            Err(IoErrorKind::NotADirectory)
        );
        assert_eq!(
            file_system.rename(Path::new("/serial"), Path::new("/uart")),
            Err(IoErrorKind::DirectoryNotEmpty)
        );
    }

    #[test]
    fn directory_listing_includes_dot_and_dot_dot_and_attribute_context() {
        let file_system = FileSystem::new();
        file_system.create_directory(Path::new("/uart")).unwrap();
        file_system.create_file(Path::new("/uart/uart0")).unwrap();

        let mut root_context = Context::new_empty();
        file_system
            .lookup_directory(&mut root_context, Path::ROOT)
            .unwrap();

        let mut root_names = Vec::new();
        while let Some(entry) = DirectoryOperations::read(&file_system, &mut root_context).unwrap()
        {
            root_names.push(entry.name);
        }
        assert_eq!(
            root_names,
            Vec::from([String::from("."), String::from(".."), String::from("uart")])
        );

        let mut directory_attributes = Attributes::new().set_mask(AttributeFlags::Kind);
        AttributeOperations::get_attributes(
            &file_system,
            &mut root_context,
            &mut directory_attributes,
        )
        .unwrap();
        assert_eq!(directory_attributes.get_kind(), Some(&Kind::Directory));

        let mut cloned_context =
            BaseOperations::clone_context(&file_system, &root_context).unwrap();
        DirectoryOperations::close(&file_system, &mut root_context).unwrap();
        DirectoryOperations::rewind(&file_system, &mut cloned_context).unwrap();
        assert_eq!(
            DirectoryOperations::read(&file_system, &mut cloned_context)
                .unwrap()
                .unwrap()
                .name,
            "."
        );
        DirectoryOperations::close(&file_system, &mut cloned_context).unwrap();
    }

    #[test]
    fn unmount_clears_state_and_mount_reinitializes() {
        let file_system = FileSystem::new();
        file_system
            .create_file(Path::new("/temporary_node"))
            .unwrap();

        file_system.mount().unwrap();
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
        FileSystemOperations::get_attributes(
            &file_system,
            Path::new("/temporary_node"),
            &mut attributes,
        )
        .unwrap();

        file_system.unmount().unwrap();
        assert_eq!(
            file_system.create_file(Path::new("/after_unmount")),
            Err(IoErrorKind::DeviceUnavailable)
        );
        assert_eq!(file_system.unmount(), Err(IoErrorKind::DeviceUnavailable));

        file_system.mount().unwrap();
        assert_eq!(
            FileSystemOperations::get_attributes(
                &file_system,
                Path::new("/temporary_node"),
                &mut attributes,
            ),
            Err(IoErrorKind::NotFound)
        );
    }

    #[test]
    fn directory_position_and_stale_generation_after_remove() {
        let file_system = FileSystem::new();
        file_system.create_directory(Path::new("/devices")).unwrap();
        file_system
            .create_file(Path::new("/devices/first"))
            .unwrap();
        file_system
            .create_file(Path::new("/devices/second"))
            .unwrap();

        let mut context = Context::new_empty();
        file_system
            .lookup_directory(&mut context, Path::new("/devices"))
            .unwrap();

        assert_eq!(
            DirectoryOperations::read(&file_system, &mut context)
                .unwrap()
                .unwrap()
                .name,
            "."
        );
        let after_dot = DirectoryOperations::get_position(&file_system, &mut context).unwrap();
        assert_eq!(
            DirectoryOperations::read(&file_system, &mut context)
                .unwrap()
                .unwrap()
                .name,
            ".."
        );
        assert_eq!(
            DirectoryOperations::read(&file_system, &mut context)
                .unwrap()
                .unwrap()
                .name,
            "first"
        );
        file_system.remove(Path::new("/devices/second")).unwrap();
        assert_eq!(
            DirectoryOperations::read(&file_system, &mut context).unwrap(),
            None
        );

        DirectoryOperations::set_position(&file_system, &mut context, after_dot).unwrap();
        assert_eq!(
            DirectoryOperations::read(&file_system, &mut context)
                .unwrap()
                .unwrap()
                .name,
            ".."
        );
        DirectoryOperations::close(&file_system, &mut context).unwrap();

        // Removing a non-empty directory fails.
        assert_eq!(
            file_system.remove(Path::new("/devices")),
            Err(IoErrorKind::DirectoryNotEmpty)
        );

        // Removing an empty directory invalidates open handles even if its slot is reused.
        file_system
            .create_directory(Path::new("/transient"))
            .unwrap();
        let mut stale_context = Context::new_empty();
        file_system
            .lookup_directory(&mut stale_context, Path::new("/transient"))
            .unwrap();
        file_system.remove(Path::new("/transient")).unwrap();
        file_system.create_directory(Path::new("/reused")).unwrap();
        assert_eq!(
            DirectoryOperations::read(&file_system, &mut stale_context),
            Err(IoErrorKind::NotFound)
        );
    }

    #[test]
    fn set_attributes_validates_kind_transitions_and_path_errors() {
        let file_system = FileSystem::new();
        file_system.create_directory(Path::new("/folder")).unwrap();
        file_system.create_file(Path::new("/folder/node")).unwrap();

        let mut attributes = Attributes::new().set_kind(Kind::File);
        assert_eq!(
            FileSystemOperations::set_attributes(&file_system, Path::new("/folder"), &attributes),
            Err(IoErrorKind::IsADirectory)
        );

        attributes = Attributes::new().set_kind(Kind::Directory);
        assert_eq!(
            FileSystemOperations::set_attributes(
                &file_system,
                Path::new("/folder/node"),
                &attributes
            ),
            Err(IoErrorKind::InvalidArgument)
        );

        attributes = Attributes::new().set_kind(Kind::Pipe).set_inode(5);
        FileSystemOperations::set_attributes(&file_system, Path::new("/folder/node"), &attributes)
            .unwrap();

        let mut read_back =
            Attributes::new().set_mask(AttributeFlags::Kind | AttributeFlags::Inode);
        FileSystemOperations::get_attributes(
            &file_system,
            Path::new("/folder/./../folder/node"),
            &mut read_back,
        )
        .unwrap();
        assert_eq!(read_back.get_kind(), Some(&Kind::Pipe));
        assert_eq!(read_back.get_inode(), Some(&5));

        assert_eq!(
            FileSystemOperations::get_attributes(
                &file_system,
                Path::new("/folder/node/"),
                &mut read_back,
            ),
            Err(IoErrorKind::NotADirectory)
        );
        assert_eq!(
            file_system.create_file(Path::new("/missing/node")),
            Err(IoErrorKind::NotFound)
        );
        assert_eq!(
            file_system.create_file(Path::new("/folder/node/child")),
            Err(IoErrorKind::NotADirectory)
        );
        // An empty path is the root, as for a mount point's own directory.
        assert_eq!(
            file_system.create_file(Path::EMPTY),
            Err(IoErrorKind::IsADirectory)
        );
        assert_eq!(
            file_system.create_directory(Path::EMPTY),
            Err(IoErrorKind::AlreadyExists)
        );
        assert_eq!(
            file_system.create_file(Path::new("/bad\0name")),
            Err(IoErrorKind::InvalidArgument)
        );
    }

    #[test]
    fn directory_entries_carry_inode_and_modification_time() {
        let file_system = FileSystem::new();
        let root_mtime = file_system::Time::new(1_700_000_000);
        let dir_mtime = file_system::Time::new(1_700_000_100);
        let file_mtime = file_system::Time::new(1_700_000_200);

        FileSystemOperations::set_attributes(
            &file_system,
            Path::ROOT,
            &Attributes::new().set_inode(1).set_modification(root_mtime),
        )
        .unwrap();
        file_system.create_directory(Path::new("/dir")).unwrap();
        FileSystemOperations::set_attributes(
            &file_system,
            Path::new("/dir"),
            &Attributes::new().set_inode(2).set_modification(dir_mtime),
        )
        .unwrap();
        file_system.create_file(Path::new("/dir/item")).unwrap();
        FileSystemOperations::set_attributes(
            &file_system,
            Path::new("/dir/item"),
            &Attributes::new().set_inode(3).set_modification(file_mtime),
        )
        .unwrap();

        let mut context = Context::new_empty();
        file_system
            .lookup_directory(&mut context, Path::new("/dir"))
            .unwrap();
        let mut entries = alloc::vec::Vec::new();
        while let Some(entry) = DirectoryOperations::read(&file_system, &mut context).unwrap() {
            entries.push(entry);
        }
        DirectoryOperations::close(&file_system, &mut context).unwrap();

        let dot = entries.iter().find(|entry| entry.name == ".").unwrap();
        assert_eq!(dot.inode, 2);
        assert_eq!(dot.modification, dir_mtime);

        let dot_dot = entries.iter().find(|entry| entry.name == "..").unwrap();
        assert_eq!(dot_dot.inode, 1);
        assert_eq!(dot_dot.modification, root_mtime);

        let item = entries.iter().find(|entry| entry.name == "item").unwrap();
        assert_eq!(item.inode, 3);
        assert_eq!(item.modification, file_mtime);
    }
}
