use crate::{
    Attributes, Context, DirectoryOperations, FileOperations, Flags, MountOperations, Path, Result,
};

pub trait FileSystemOperations: FileOperations + DirectoryOperations + MountOperations {
    /// Return the fields this backend can accept through `set_attributes`.
    ///
    /// This is a capability hint for VFS-managed default metadata during file
    /// creation and open. Explicit attribute mutations still call
    /// `set_attributes` and receive the backend's normal errors. Existing
    /// implementations default to the historical all-fields behavior.
    fn writable_attributes(&self) -> crate::AttributeFlags {
        crate::AttributeFlags::all()
    }

    fn lookup_directory(&self, context: &mut Context, path: &Path) -> Result<()>;

    fn lookup_file(&self, context: &mut Context, path: &Path, flags: Flags) -> Result<()>;

    fn create_directory(&self, path: &Path) -> Result<()>;

    fn create_file(&self, path: &Path) -> Result<()>;

    /// Remove a file or directory from the file system.
    ///
    /// Permanently deletes the specified file or directory. For directories,
    /// they must be empty before they can be removed.
    ///
    /// # Arguments
    ///
    /// * `context` - File system context
    /// * `path` - Path to the file or directory to remove
    ///
    /// # Returns
    ///
    /// * `Ok(())` - File or directory successfully removed
    /// * `Err(Error)` - Error during removal
    ///
    /// # Errors
    ///
    /// * [`crate::Error::NotFound`] - File or directory doesn't exist
    /// * [`crate::Error::PermissionDenied`] - Insufficient permissions
    /// * [`crate::Error::DirectoryNotEmpty`] - Directory contains files
    /// * [`crate::Error::RessourceBusy`] - File is currently in use
    fn remove(&self, path: &Path) -> Result<()>;

    /// Rename or move a file or directory.
    ///
    /// Changes the name or location of a file or directory. This can be used
    /// for both renaming within the same directory and moving between directories.
    ///
    /// # Arguments
    ///
    /// * `context` - File system context
    /// * `source` - Current path of the file or directory
    /// * `destination` - New path for the file or directory
    ///
    /// # Returns
    ///
    /// * `Ok(())` - File or directory successfully renamed/moved
    /// * `Err(Error)` - Error during rename operation
    ///
    /// # Errors
    ///
    /// * [`crate::Error::NotFound`] - Source file doesn't exist
    /// * [`crate::Error::AlreadyExists`] - Destination already exists
    /// * [`crate::Error::PermissionDenied`] - Insufficient permissions
    fn rename(&self, source: &Path, destination: &Path) -> Result<()>;

    // - Directory

    // - Statistics

    /// Populate the requested attributes for a path.
    ///
    /// On entry, `attributes.get_mask()` selects the fields requested by the
    /// caller. On success, the mask must contain only fields that were both
    /// requested and populated with meaningful values. Backends may omit
    /// unavailable fields; they must not fabricate values to satisfy a request.
    /// An empty mask is a successful no-op.
    fn get_attributes(&self, path: &Path, attributes: &mut Attributes) -> Result<()>;

    /// Change the fields selected by `attributes.get_mask()` for a path.
    ///
    /// An empty mask is a successful no-op. If any requested field is
    /// unsupported, the operation must return [`crate::Error::UnsupportedOperation`]
    /// before changing any supported field. A later backend I/O or permission
    /// failure may occur after an earlier supported field was changed; this
    /// operation does not promise rollback for such failures.
    fn set_attributes(&self, path: &Path, attributes: &Attributes) -> Result<()>;
}

pub mod tests {
    //! Generic test suite for file system operations.
    //!
    //! This module provides comprehensive tests for any implementation of the
    //! `FileSystemOperations` trait. It includes tests for:
    //!
    //! - File operations (create, lookup, remove)
    //! - Directory operations (create, lookup, remove, nested)
    //! - Rename and move operations
    //! - Attribute get/set operations
    //! - Error handling and edge cases
    //! - File system consistency
    //! - Special cases (root directory, long names, special characters)
    //!
    //! # Usage
    //!
    //! ## Using the Macro (Recommended)
    //!
    //! The easiest way to test your file system implementation is to use the
    //! `implement_file_system_tests!` macro, which generates individual test
    //! functions for each test category:
    //!
    //! ```rust,ignore
    //! use file_system::implement_file_system_tests;
    //!
    //! mod tests {
    //!     use super::*;
    //!     
    //!     implement_file_system_tests! {
    //!         instance: MyFileSystem::new()
    //!     }
    //! }
    //! ```
    //!
    //! This will generate 32+ individual `#[test]` functions, each testing a specific
    //! aspect of your file system implementation.
    //!
    //! ## Using Individual Test Functions
    //!
    //! You can also call individual test functions directly:
    //!
    //! ```rust,ignore
    //! use file_system::operations::file_system::tests;
    //!
    //! #[test]
    //! fn test_my_file_system() {
    //!     let fs = MyFileSystem::new();
    //!     
    //!     // Run all tests
    //!     tests::run_all_tests(&fs);
    //!     
    //!     // Or run individual test categories
    //!     tests::test_file_operations(&fs);
    //!     tests::test_directory_operations(&fs);
    //!     tests::test_rename_operations(&fs);
    //! }
    //! ```
    //!
    //! # Test Categories
    //!
    //! ## File System Structure Tests
    //! - `test_file_operations` - Basic file CRUD operations
    //! - `test_directory_operations` - Basic directory operations
    //! - `test_nested_directories` - Nested directory hierarchy
    //! - `test_rename_operations` - File/directory rename and move
    //! - `test_attribute_operations` - Metadata get/set operations
    //!
    //! ## File I/O Tests
    //! - `test_file_read_operations` - Reading data from files
    //! - `test_file_write_operations` - Writing data to files
    //! - `test_file_write_pattern` - Pattern-based writing
    //! - `test_file_write_vectored` - Vectored (scatter-gather) I/O
    //! - `test_file_position_operations` - Position-based read/write
    //! - `test_file_read_until` - Reading until delimiter
    //! - `test_file_flush` - Flushing buffered data
    //! - `test_large_file_operations` - Large file handling
    //!
    //! ## Directory I/O Tests
    //! - `test_directory_read_operations` - Reading directory entries
    //! - `test_directory_position_operations` - Directory position management
    //! - `test_directory_rewind` - Rewinding directory stream
    //! - `test_empty_directory_read` - Reading empty directories
    //!
    //! ## Error Handling & Edge Cases
    //! - `test_error_handling` - Error case validation
    //! - `test_file_system_consistency` - Consistency checks
    //! - `test_root_directory_operations` - Root directory special cases
    //! - `test_invalid_paths` - Invalid path handling
    //!
    //! ## Test Execution
    //! - `implement_file_system_tests!` - Macro to generate all test functions (recommended)
    //! - `run_all_tests` - Function to execute all test categories sequentially

    use super::*;
    use crate::{
        AccessFlags, AttributeFlags, Attributes, BaseOperations, Context, Error, Kind, Permissions,
        Size, Time,
    };
    use users::{GroupIdentifier, UserIdentifier};

    /// Attribute capabilities used by the generic test suite.
    ///
    /// This is test configuration only; it is not a runtime query on
    /// [`FileSystemOperations`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FileSystemTestCapabilities {
        pub readable: AttributeFlags,
        pub writable: AttributeFlags,
    }

    impl Default for FileSystemTestCapabilities {
        fn default() -> Self {
            Self {
                readable: AttributeFlags::all(),
                writable: AttributeFlags::all(),
            }
        }
    }

    /// Generic test suite for file system operations.
    ///
    /// This provides a comprehensive set of tests that can be used to verify
    /// any implementation of the `FileSystemOperations` trait.
    pub fn test_file_system_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        test_file_operations(fs);
        test_directory_operations(fs);
        test_attribute_operations(fs);
        test_error_handling(fs);
    }

    /// Test basic file operations: create, lookup, remove
    pub fn test_file_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_file = Path::new("/test_file.txt");
        let mut context = Context::new_empty();

        // Test file creation
        fs.create_file(test_file).unwrap();

        // Test file lookup
        fs.lookup_file(&mut context, test_file, AccessFlags::Read.into())
            .unwrap();

        // Test file removal
        fs.remove(test_file).unwrap();

        // Test lookup after removal should fail
        fs.lookup_file(&mut context, test_file, AccessFlags::Read.into())
            .unwrap_err();
    }

    /// Test file creation with various edge cases
    pub fn test_file_creation_edge_cases<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        // Test creating file that already exists
        let test_file = Path::new("/duplicate.txt");
        fs.create_file(test_file).unwrap();

        let result = fs.create_file(test_file);
        assert!(
            matches!(result, Err(Error::AlreadyExists)),
            "Creating duplicate file should return AlreadyExists error"
        );

        // Cleanup
        fs.remove(test_file).unwrap();
    }

    /// Test directory operations: create, lookup, remove
    pub fn test_directory_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_dir = Path::new("/test_directory");
        let mut context = Context::new_empty();

        // Test directory creation
        fs.create_directory(test_dir).unwrap();

        // Test directory lookup
        fs.lookup_directory(&mut context, test_dir).unwrap();

        DirectoryOperations::close(fs, &mut context).unwrap();

        // Test directory removal
        fs.remove(test_dir).unwrap();

        let mut context = Context::new_empty();

        // Test lookup after removal should fail
        fs.lookup_directory(&mut context, test_dir).unwrap_err();
    }

    /// Test nested directory operations
    pub fn test_nested_directories<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let parent_dir = Path::new("/parent");
        let child_dir = Path::new("/parent/child");

        // Create parent directory
        fs.create_directory(parent_dir).unwrap();

        // Create child directory
        fs.create_directory(child_dir).unwrap();

        // Try to remove parent with child (should fail)
        let result = fs.remove(parent_dir);
        assert!(
            matches!(result, Err(Error::DirectoryNotEmpty)),
            "Removing non-empty directory should fail with DirectoryNotEmpty"
        );

        // Remove child first
        fs.remove(child_dir).unwrap();

        // Now parent removal should succeed
        fs.remove(parent_dir).unwrap();
    }

    /// Test rename/move operations
    pub fn test_rename_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let source = Path::new("/source_file.txt");
        let destination = Path::new("/destination_file.txt");
        let mut context = Context::new_empty();

        // Create source file
        fs.create_file(source).unwrap();

        // Test rename
        fs.rename(source, destination).unwrap();

        // Verify source no longer exists
        fs.lookup_file(&mut context, source, AccessFlags::Read.into())
            .unwrap_err();

        // Verify destination exists
        assert!(
            fs.lookup_file(&mut context, destination, AccessFlags::Read.into())
                .is_ok(),
            "Destination file should exist after rename"
        );

        // Cleanup
        fs.remove(destination).unwrap();
    }

    /// Test rename edge cases
    pub fn test_rename_edge_cases<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let source = Path::new("/rename_source.txt");
        let destination = Path::new("/rename_dest.txt");

        // Test renaming non-existent file
        let result = fs.rename(source, destination);
        assert_eq!(result, Err(Error::NotFound),);

        // Create both files
        fs.create_file(source).unwrap();
        fs.create_file(destination).unwrap();

        // Test renaming to existing file
        //let _ = fs.rename(source, destination);
        //assert_eq!(
        //    result,
        //    Err(Error::AlreadyExists),
        //);

        // Cleanup
        fs.remove(source).unwrap();
        fs.remove(destination).unwrap();
    }

    /// Test moving files between directories
    pub fn test_move_between_directories<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let dir1 = Path::new("/dir1");
        let dir2 = Path::new("/dir2");
        let file_in_dir1 = Path::new("/dir1/file.txt");
        let file_in_dir2 = Path::new("/dir2/file.txt");

        // Create directories
        fs.create_directory(dir1).unwrap();
        fs.create_directory(dir2).unwrap();

        // Create file in dir1
        fs.create_file(file_in_dir1).unwrap();

        // Move file to dir2
        fs.rename(file_in_dir1, file_in_dir2).unwrap();

        // Verify file is in dir2
        let mut context = Context::new_empty();
        fs.lookup_file(&mut context, file_in_dir2, AccessFlags::Read.into())
            .unwrap();

        // Cleanup
        fs.remove(file_in_dir2).unwrap();
        fs.remove(dir2).unwrap();
        fs.remove(dir1).unwrap();
    }

    /// Test attribute get/set operations
    pub fn test_attribute_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        test_attribute_operations_with_capabilities(fs, FileSystemTestCapabilities::default());
    }

    /// Test attribute operations for a backend with explicitly configured
    /// readable and writable fields.
    pub fn test_attribute_operations_with_capabilities<F>(
        fs: &F,
        capabilities: FileSystemTestCapabilities,
    ) where
        F: FileSystemOperations,
    {
        let test_file = Path::new("/attr_test.txt");
        fs.create_file(test_file).unwrap();

        let writable = capabilities.writable.intersection(AttributeFlags::all());
        let readable = capabilities.readable.intersection(AttributeFlags::all());
        let expected = Attributes::new()
            .set_size(0)
            .set_permissions(Permissions::ALL_READ_WRITE)
            .set_kind(Kind::File)
            .set_user(UserIdentifier::new(42))
            .set_group(GroupIdentifier::new(43))
            .set_inode(42)
            .set_creation(Time::new(1))
            .set_modification(Time::new(2))
            .set_access(Time::new(3))
            .set_status(Time::new(4));

        let mut requested_write = expected.clone().set_mask(writable);
        FileSystemOperations::set_attributes(fs, test_file, &requested_write).unwrap();

        // Empty masks are no-ops.
        requested_write = requested_write.set_mask(AttributeFlags::empty());
        FileSystemOperations::set_attributes(fs, test_file, &requested_write).unwrap();

        let mut actual = Attributes::new().set_mask(readable);
        FileSystemOperations::get_attributes(fs, test_file, &mut actual).unwrap();
        assert_eq!(
            actual.get_mask(),
            readable,
            "backend returned an inaccurate attribute mask"
        );
        assert_attribute_values(&expected, &actual, readable.intersection(writable));
        if readable.contains(AttributeFlags::Size) {
            assert_eq!(
                actual.get_size(),
                Some(&0),
                "new file should have size zero"
            );
        }

        let subset = readable.intersection(AttributeFlags::Kind | AttributeFlags::Size);
        if !subset.is_empty() {
            let mut subset_result = Attributes::new().set_mask(subset);
            FileSystemOperations::get_attributes(fs, test_file, &mut subset_result).unwrap();
            assert_eq!(
                subset_result.get_mask(),
                subset,
                "backend ignored the requested read mask"
            );
            if subset.contains(AttributeFlags::Kind) {
                assert_eq!(subset_result.get_kind(), Some(&Kind::File));
            }
            if subset.contains(AttributeFlags::Size) {
                assert_eq!(subset_result.get_size(), Some(&0));
            }
        }

        let unsupported = AttributeFlags::all().difference(writable);
        if !unsupported.is_empty() {
            let changed = Attributes::new()
                .set_permissions(Permissions::NONE)
                .set_kind(Kind::Directory)
                .set_user(UserIdentifier::ROOT)
                .set_group(GroupIdentifier::ROOT)
                .set_inode(99)
                .set_creation(Time::new(11))
                .set_modification(Time::new(12))
                .set_access(Time::new(13))
                .set_status(Time::new(14))
                .set_mask(AttributeFlags::all());
            assert!(matches!(
                FileSystemOperations::set_attributes(fs, test_file, &changed),
                Err(Error::UnsupportedOperation)
            ));

            let mut after_rejection = Attributes::new().set_mask(readable);
            FileSystemOperations::get_attributes(fs, test_file, &mut after_rejection).unwrap();
            assert_eq!(after_rejection.get_mask(), readable);
            assert_attribute_values(&expected, &after_rejection, readable.intersection(writable));
        }

        fs.remove(test_file).unwrap();
    }

    fn assert_attribute_values(expected: &Attributes, actual: &Attributes, mask: AttributeFlags) {
        macro_rules! assert_field {
            ($flag:expr, $getter:ident) => {
                if mask.contains($flag) {
                    assert_eq!(actual.$getter(), expected.$getter());
                }
            };
        }

        assert_field!(AttributeFlags::Inode, get_inode);
        assert_field!(AttributeFlags::Kind, get_kind);
        assert_field!(AttributeFlags::Size, get_size);
        assert_field!(AttributeFlags::Links, get_links);
        assert_field!(AttributeFlags::CreationTime, get_creation);
        assert_field!(AttributeFlags::ModificationTime, get_modification);
        assert_field!(AttributeFlags::AccessTime, get_access);
        assert_field!(AttributeFlags::StatusTime, get_status);
        assert_field!(AttributeFlags::Permissions, get_permissions);
        assert_field!(AttributeFlags::User, get_user);
        assert_field!(AttributeFlags::Group, get_group);
    }

    /// Test attribute operations on directories
    pub fn test_directory_attributes<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_dir = Path::new("/attr_dir");

        // Create test directory
        fs.create_directory(test_dir).unwrap();

        // Get directory attributes
        let mut attrs = Attributes::new().set_mask(AttributeFlags::Kind);
        FileSystemOperations::get_attributes(fs, test_dir, &mut attrs).unwrap();

        // Verify it's a directory
        if let Some(kind) = attrs.get_kind() {
            assert_eq!(*kind, Kind::Directory, "Should be a directory type");
        }

        // Cleanup
        fs.remove(test_dir).unwrap();
    }

    /// Test error handling for various scenarios
    pub fn test_error_handling<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let mut context = Context::new_empty();

        // Test NotFound errors
        let nonexistent = Path::new("/nonexistent.txt");
        fs.lookup_file(&mut context, nonexistent, AccessFlags::Read.into())
            .unwrap_err();

        assert!(
            fs.remove(nonexistent).is_err(),
            "Should return error when removing non-existent file"
        );

        // Test getting attributes of non-existent file
        let mut attrs = Attributes::new().set_mask(AttributeFlags::all());
        FileSystemOperations::get_attributes(fs, nonexistent, &mut attrs).unwrap_err();
    }

    /// Test invalid path handling
    pub fn test_invalid_paths<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        // Note: Path validation typically happens at Path::new(),
        // but we test file system behavior with edge cases

        // Test empty filename (directory operations might handle this differently)
        let path = Path::new("/.txt");
        // If path is valid, file system should handle it appropriately
        let file_result = fs.create_file(path);
        // Result depends on implementation - may succeed or fail
        if file_result.is_ok() {
            fs.remove(path).unwrap();
        }
    }

    /// Test concurrent operations (if file system supports it)
    pub fn test_multiple_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        // Create multiple files
        let files = [
            "/multi_test_1.txt",
            "/multi_test_2.txt",
            "/multi_test_3.txt",
        ];

        for file_path in &files {
            let path = Path::new(file_path);
            fs.create_file(path).unwrap();
        }

        // Verify all files exist
        for file_path in &files {
            let path = Path::new(file_path);
            let mut context = Context::new_empty();
            fs.lookup_file(&mut context, path, AccessFlags::Read.into())
                .unwrap();
            BaseOperations::close(fs, &mut context).unwrap();
        }

        // Remove all files
        for file_path in &files {
            let path = Path::new(file_path);
            fs.remove(path).unwrap();
        }
    }

    /// Test file system consistency
    pub fn test_file_system_consistency<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let dir = Path::new("/consistency_test");
        let file = Path::new("/consistency_test/file.txt");

        // Create directory and file
        fs.create_directory(dir).unwrap();
        fs.create_file(file).unwrap();

        // Attempt to create file with same name as directory should fail
        let result = fs.create_file(dir);
        result.unwrap_err();

        // Attempt to create directory with same name as file should fail
        let result = fs.create_directory(file);
        result.unwrap_err();

        // Cleanup
        fs.remove(file).unwrap();
        fs.remove(dir).unwrap();
    }

    /// Test file lookup with different flags
    pub fn test_lookup_with_flags<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_file = Path::new("/flags_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();

        // Test with different flag combinations
        fs.lookup_file(&mut context, test_file, AccessFlags::Read.into())
            .unwrap();
        let mut context = Context::new_empty();

        fs.lookup_file(&mut context, test_file, AccessFlags::Write.into())
            .unwrap();
        let mut context = Context::new_empty();

        fs.lookup_file(&mut context, test_file, AccessFlags::READ_WRITE.into())
            .unwrap();

        // Cleanup
        fs.remove(test_file).unwrap();
    }

    /// Test operations on root directory
    pub fn test_root_directory_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let root = Path::new("/");
        let mut context = Context::new_empty();

        // Root should always exist
        fs.lookup_directory(&mut context, root).unwrap();

        // Should not be able to remove root
        let result = fs.remove(root);
        result.unwrap_err();

        // Should be able to get root attributes
        let mut attrs = Attributes::new().set_mask(AttributeFlags::all());
        FileSystemOperations::get_attributes(fs, root, &mut attrs).unwrap();
    }

    /// Test long file names
    pub fn test_long_filenames<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        // Create a reasonably long filename
        let long_name = "/this_is_a_very_long_filename_that_tests_the_limits.txt";
        let path = Path::new(long_name);

        let result = fs.create_file(path);
        // May succeed or fail depending on file system limits
        if result.is_ok() {
            fs.remove(path).unwrap();
        }
    }

    /// Test special characters in filenames
    pub fn test_special_characters_in_names<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let special_chars = [
            "/file-with-dash.txt",
            "/file_with_underscore.txt",
            "/file.multiple.dots.txt",
        ];

        for name in &special_chars {
            let path = Path::new(name);
            let result = fs.create_file(path);
            if result.is_ok() {
                fs.remove(path).unwrap();
            }
        }
    }

    /// Test file read operations (when FileOperations::read is available)
    pub fn test_file_read_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/read_test.txt");
        let mut context = Context::new_empty();

        // Create and open file for writing
        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::Write.into())
            .unwrap();

        // Write test data
        let test_data = b"Hello, File System!";
        let write_result = fs.write_sync(&mut context, test_data, 0);
        write_result.as_ref().unwrap();
        assert_eq!(
            write_result.unwrap(),
            test_data.len(),
            "Should write all bytes"
        );

        // Close and reopen for reading
        BaseOperations::close(fs, &mut context).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::Read.into())
            .unwrap();

        // Read data back
        let mut buffer = alloc::vec![0u8; test_data.len()];
        let read_result = BaseOperations::read_sync(fs, &mut context, &mut buffer, 0);
        read_result.as_ref().unwrap();
        assert_eq!(
            read_result.unwrap(),
            test_data.len(),
            "Should read all bytes"
        );
        assert_eq!(
            &buffer[..],
            test_data,
            "Read data should match written data"
        );

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test file write operations with various patterns
    pub fn test_file_write_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/write_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::Write.into())
            .unwrap();

        // Test single write
        let data1 = b"First write. ";
        let result = BaseOperations::write_sync(fs, &mut context, data1, 0);
        result.unwrap();

        // Test sequential write
        let data2 = b"Second write.";
        let result = BaseOperations::write_sync(fs, &mut context, data2, data1.len() as Size);
        result.unwrap();

        // Test overwrite
        let overwrite_data = b"OVERWRITE";
        let result = BaseOperations::write_sync(fs, &mut context, overwrite_data, 0);
        result.unwrap();

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test file write pattern operations
    pub fn test_file_write_pattern<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/pattern_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::Write.into())
            .unwrap();

        // Write a pattern multiple times
        let pattern = b"ABC";
        let count = 5;
        let result = BaseOperations::write_pattern_sync(fs, &mut context, pattern, count, 0);

        if let Ok(bytes_written) = result {
            assert!(
                bytes_written <= pattern.len() * count,
                "Should not write more than requested"
            );

            // Read back and verify pattern
            BaseOperations::close(fs, &mut context).unwrap();
            fs.lookup_file(&mut context, test_file, AccessFlags::Read.into())
                .unwrap();

            let mut buffer = alloc::vec![0u8; bytes_written];
            let read_result = BaseOperations::read_sync(fs, &mut context, &mut buffer, 0);
            read_result.unwrap();
        }

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test file write vectored operations
    pub fn test_file_write_vectored<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/vectored_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::Write.into())
            .unwrap();

        // Write multiple buffers at once
        let buf1 = b"First ";
        let buf2 = b"Second ";
        let buf3 = b"Third";
        let buffers = [buf1.as_slice(), buf2.as_slice(), buf3.as_slice()];

        let result = BaseOperations::write_vectored_sync(fs, &mut context, &buffers, 0);
        if let Ok(total_written) = result {
            let expected_total = buf1.len() + buf2.len() + buf3.len();
            assert!(
                total_written <= expected_total,
                "Should write all buffers or partial"
            );

            // Read back and verify
            BaseOperations::close(fs, &mut context).unwrap();
            fs.lookup_file(&mut context, test_file, AccessFlags::Read.into())
                .unwrap();

            let mut buffer = alloc::vec![0u8; total_written];
            let read_result = BaseOperations::read_sync(fs, &mut context, &mut buffer, 0);
            read_result.unwrap();
        }

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test file read/write at specific positions
    pub fn test_file_position_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/position_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::READ_WRITE.into())
            .unwrap();

        // Write data at different positions
        let data1 = b"AAAA";
        let data2 = b"BBBB";
        let data3 = b"CCCC";

        BaseOperations::write_sync(fs, &mut context, data1, 0).unwrap();
        BaseOperations::write_sync(fs, &mut context, data2, 4).unwrap();
        BaseOperations::write_sync(fs, &mut context, data3, 8).unwrap();

        // Read from specific positions
        let mut buffer = alloc::vec![0u8; 4];

        let result = BaseOperations::read_sync(fs, &mut context, &mut buffer, 0);
        result.unwrap();
        assert_eq!(&buffer[..], b"AAAA", "Should read first chunk");

        let result = BaseOperations::read_sync(fs, &mut context, &mut buffer, 4);
        result.unwrap();
        assert_eq!(&buffer[..], b"BBBB", "Should read second chunk");

        let result = BaseOperations::read_sync(fs, &mut context, &mut buffer, 8);
        result.unwrap();
        assert_eq!(&buffer[..], b"CCCC", "Should read third chunk");

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test file read until delimiter
    pub fn test_file_read_until<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/read_until_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::READ_WRITE.into())
            .unwrap();

        // Write data with delimiter
        let test_data = b"Line 1\nLine 2\nLine 3\n";
        BaseOperations::write_sync(fs, &mut context, test_data, 0).unwrap();

        // Read until newline
        let mut buffer = alloc::vec![0u8; 100];
        let result = BaseOperations::read_until_sync(fs, &mut context, &mut buffer, 0, b"\n");

        if let Ok(bytes_read) = result {
            assert!(bytes_read > 0, "Should read some bytes");
            assert!(bytes_read <= 7, "Should stop at first newline");
        }

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test file flush operation
    pub fn test_file_flush<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/flush_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::Write.into())
            .unwrap();

        // Write data
        let data = b"Data to flush";
        BaseOperations::write_sync(fs, &mut context, data, 0).unwrap();

        // Flush should succeed or be unsupported
        let result = BaseOperations::flush_sync(fs, &mut context);
        assert!(
            result.is_ok() || matches!(result, Err(Error::UnsupportedOperation)),
            "Flush should succeed or be unsupported"
        );

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test empty file operations
    pub fn test_empty_file_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/empty_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::Read.into())
            .unwrap();

        // Try to read from empty file
        let mut buffer = alloc::vec![0u8; 100];
        let result = BaseOperations::read_sync(fs, &mut context, &mut buffer, 0);

        // Should either read 0 bytes or return an error
        if let Ok(bytes_read) = result {
            assert_eq!(bytes_read, 0, "Should read 0 bytes from empty file");
        }

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test large file operations
    pub fn test_large_file_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        use crate::BaseOperations;

        let test_file = Path::new("/large_test.txt");
        let mut context = Context::new_empty();

        fs.create_file(test_file).unwrap();
        fs.lookup_file(&mut context, test_file, AccessFlags::READ_WRITE.into())
            .unwrap();

        // Write a larger amount of data
        let large_data = alloc::vec![0xABu8; 4096];
        let write_result = BaseOperations::write_sync(fs, &mut context, &large_data, 0);

        if let Ok(bytes_written) = write_result {
            // Read back the data
            let mut read_buffer = alloc::vec![0u8; bytes_written];
            let read_result = BaseOperations::read_sync(fs, &mut context, &mut read_buffer, 0);

            if let Ok(bytes_read) = read_result {
                assert_eq!(
                    bytes_read, bytes_written,
                    "Should read back same amount written"
                );
                assert_eq!(
                    &read_buffer[..bytes_read],
                    &large_data[..bytes_written],
                    "Read data should match written data"
                );
            }
        }

        // Cleanup
        BaseOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
    }

    /// Test directory read operations
    pub fn test_directory_read_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_dir = Path::new("/read_dir_test");
        let file1 = Path::new("/read_dir_test/file1.txt");
        let file2 = Path::new("/read_dir_test/file2.txt");
        let file3 = Path::new("/read_dir_test/file3.txt");
        let mut context = Context::new_empty();

        // Create directory and files
        fs.create_directory(test_dir).unwrap();
        fs.create_file(file1).unwrap();
        fs.create_file(file2).unwrap();
        fs.create_file(file3).unwrap();

        // Open directory for reading
        fs.lookup_directory(&mut context, test_dir).unwrap();

        // Read directory entries
        let mut entry_count = 0;
        let mut found_files = alloc::vec::Vec::new();

        loop {
            let result = DirectoryOperations::read(fs, &mut context);
            match result {
                Ok(Some(entry)) => {
                    entry_count += 1;
                    found_files.push(entry.name.clone());
                }
                Ok(None) => break, // End of directory
                Err(_) => break,   // Error reading
            }

            // Safety limit to prevent infinite loops
            if entry_count > 100 {
                break;
            }
        }

        // Should find at least the files we created (may also include . and ..)
        assert!(entry_count >= 3, "Should find at least the 3 created files");

        // Cleanup
        DirectoryOperations::close(fs, &mut context).unwrap();
        fs.remove(file1).unwrap();
        fs.remove(file2).unwrap();
        fs.remove(file3).unwrap();
        fs.remove(test_dir).unwrap();
    }

    /// Test directory position operations
    pub fn test_directory_position_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_dir = Path::new("/pos_dir_test");
        let file1 = Path::new("/pos_dir_test/a.txt");
        let file2 = Path::new("/pos_dir_test/b.txt");
        let mut context = Context::new_empty();

        // Create directory and files
        fs.create_directory(test_dir).unwrap();
        fs.create_file(file1).unwrap();
        fs.create_file(file2).unwrap();

        // Open directory
        fs.lookup_directory(&mut context, test_dir).unwrap();

        // Read first entry
        let first_result = DirectoryOperations::read(fs, &mut context);
        first_result.unwrap();

        // Get position
        let pos_result = fs.get_position(&mut context);
        if pos_result.is_ok() {
            // Set position back to start
            let set_result = DirectoryOperations::set_position(fs, &mut context, 0);
            assert!(
                set_result.is_ok() || matches!(set_result, Err(Error::UnsupportedOperation)),
                "Should set position or be unsupported"
            );
        }

        // Test rewind
        let rewind_result = fs.rewind(&mut context);
        assert!(
            rewind_result.is_ok() || matches!(rewind_result, Err(Error::UnsupportedOperation)),
            "Should rewind or be unsupported"
        );

        // Cleanup
        DirectoryOperations::close(fs, &mut context).unwrap();
        fs.remove(file1).unwrap();
        fs.remove(file2).unwrap();
        fs.remove(test_dir).unwrap();
    }

    /// Test directory rewind and re-read
    pub fn test_directory_rewind<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_dir = Path::new("/rewind_test");
        let file = Path::new("/rewind_test/file.txt");
        let mut context = Context::new_empty();

        // Create directory and file
        fs.create_directory(test_dir).unwrap();
        fs.create_file(file).unwrap();

        // Open directory
        fs.lookup_directory(&mut context, test_dir).unwrap();

        // Read all entries
        let mut first_pass_count = 0;
        while let Ok(Some(_)) = DirectoryOperations::read(fs, &mut context) {
            first_pass_count += 1;
            if first_pass_count > 100 {
                break;
            }
        }

        // Rewind
        if fs.rewind(&mut context).is_ok() {
            // Read again
            let mut second_pass_count = 0;
            while let Ok(Some(_)) = DirectoryOperations::read(fs, &mut context) {
                second_pass_count += 1;
                if second_pass_count > 100 {
                    break;
                }
            }

            assert_eq!(
                first_pass_count, second_pass_count,
                "Should read same number of entries after rewind"
            );
        }

        // Cleanup
        DirectoryOperations::close(fs, &mut context).unwrap();
        fs.remove(file).unwrap();
        fs.remove(test_dir).unwrap();
    }

    /// Test reading empty directory
    pub fn test_empty_directory_read<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_dir = Path::new("/empty_dir");
        let mut context = Context::new_empty();

        fs.create_directory(test_dir).unwrap();
        fs.lookup_directory(&mut context, test_dir).unwrap();

        // Read from empty directory (may have . and .. entries)
        let result = DirectoryOperations::read(fs, &mut context);
        // Either returns None immediately or returns . and .. entries
        assert!(
            result.is_ok() || matches!(result, Err(Error::NotFound)),
            "Reading empty directory should succeed or return not found"
        );

        // Cleanup
        DirectoryOperations::close(fs, &mut context).unwrap();
        fs.remove(test_dir).unwrap();
    }

    /// Test directory entry metadata
    pub fn test_directory_entry_metadata<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_dir = Path::new("/metadata_dir");
        let test_file = Path::new("/metadata_dir/test.txt");
        let test_subdir = Path::new("/metadata_dir/subdir");
        let mut context = Context::new_empty();

        // Create structures
        fs.create_directory(test_dir).unwrap();
        fs.create_file(test_file).unwrap();
        fs.create_directory(test_subdir).unwrap();

        // Open and read directory
        fs.lookup_directory(&mut context, test_dir).unwrap();

        let mut found_file = false;
        let mut found_dir = false;

        for _ in 0..10 {
            match DirectoryOperations::read(fs, &mut context) {
                Ok(Some(entry)) => {
                    // Check entry has valid metadata
                    assert!(!entry.name.is_empty(), "Entry should have a name");

                    if entry.name.contains("test.txt") {
                        found_file = true;
                        // Could check that kind is File if available
                    }
                    if entry.name.contains("subdir") {
                        found_dir = true;
                        // Could check that kind is Directory if available
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert!(found_file, "Should find test file in directory");
        assert!(found_dir, "Should find subdirectory in directory");

        // Cleanup
        DirectoryOperations::close(fs, &mut context).unwrap();
        fs.remove(test_file).unwrap();
        fs.remove(test_subdir).unwrap();
        fs.remove(test_dir).unwrap();
    }

    /// Test open and close operations
    pub fn test_open_close_operations<F>(fs: &F)
    where
        F: FileSystemOperations,
    {
        let test_file = Path::new("/open_close_test.txt");
        let mut context = Context::new_empty();

        // Create file
        fs.create_file(test_file).unwrap();

        // Open file
        let open_result = fs.lookup_file(&mut context, test_file, AccessFlags::Read.into());
        open_result.unwrap();

        // Close file
        let close_result = BaseOperations::close(fs, &mut context);
        assert!(
            close_result.is_ok() || matches!(close_result, Err(Error::UnsupportedOperation)),
            "Should close file or be unsupported"
        );

        // Open directory
        let test_dir = Path::new("/open_close_dir");
        fs.create_directory(test_dir).unwrap();

        let open_result = fs.lookup_directory(&mut context, test_dir);
        open_result.unwrap();

        // Close directory
        DirectoryOperations::close(fs, &mut context).unwrap();

        // Cleanup
        fs.remove(test_file).unwrap();
        fs.remove(test_dir).unwrap();
    }

    /// Macro to implement all file system operation tests for a given file system instance.
    ///
    /// This macro generates individual test functions for each test category, making it easy
    /// to integrate comprehensive file system testing into your test suite.
    ///
    /// # Usage
    ///
    /// ```rust,ignore
    /// use file_system::implement_file_system_tests;
    ///
    /// mod tests {
    ///     use super::*;
    ///     
    ///     implement_file_system_tests! {
    ///         instance: MyFileSystem::new()
    ///     }
    /// }
    /// ```
    ///
    /// This will generate individual `#[test]` functions for each test category.
    #[macro_export]
    macro_rules! implement_file_system_tests {
        ($fs:expr) => {
            $crate::implement_file_system_tests!(@impl $fs, None);
        };
        ($fs:expr, $attribute_capabilities:expr) => {
            $crate::implement_file_system_tests!(@impl $fs, Some($attribute_capabilities));
        };
        (@impl $fs:expr, $attribute_capabilities:expr) => {
            // Basic file system structure tests
            #[test]
            fn test_file_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_file_operations(&fs);
            }

            #[test]
            fn test_file_creation_edge_cases() {
                let fs = $fs;
                $crate::file_system::tests::test_file_creation_edge_cases(&fs);
            }

            #[test]
            fn test_directory_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_directory_operations(&fs);
            }

            #[test]
            fn test_nested_directories() {
                let fs = $fs;
                $crate::file_system::tests::test_nested_directories(&fs);
            }

            #[test]
            fn test_rename_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_rename_operations(&fs);
            }

            #[test]
            fn test_rename_edge_cases() {
                let fs = $fs;
                $crate::file_system::tests::test_rename_edge_cases(&fs);
            }

            #[test]
            fn test_move_between_directories() {
                let fs = $fs;
                $crate::file_system::tests::test_move_between_directories(&fs);
            }

            // Attribute operations
            #[test]
            fn test_attribute_operations() {
                let fs = $fs;
                match $attribute_capabilities {
                    Some(capabilities) => $crate::file_system::tests::test_attribute_operations_with_capabilities(&fs, capabilities),
                    None => $crate::file_system::tests::test_attribute_operations(&fs),
                }
            }

            #[test]
            fn test_directory_attributes() {
                let fs = $fs;
                $crate::file_system::tests::test_directory_attributes(&fs);
            }

            // File I/O operations
            #[test]
            fn test_file_read_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_file_read_operations(&fs);
            }

            #[test]
            fn test_file_write_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_file_write_operations(&fs);
            }

            #[test]
            fn test_file_write_pattern() {
                let fs = $fs;
                $crate::file_system::tests::test_file_write_pattern(&fs);
            }

            #[test]
            fn test_file_write_vectored() {
                let fs = $fs;
                $crate::file_system::tests::test_file_write_vectored(&fs);
            }

            #[test]
            fn test_file_position_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_file_position_operations(&fs);
            }

            #[test]
            fn test_file_read_until() {
                let fs = $fs;
                $crate::file_system::tests::test_file_read_until(&fs);
            }

            #[test]
            fn test_file_flush() {
                let fs = $fs;
                $crate::file_system::tests::test_file_flush(&fs);
            }

            #[test]
            fn test_empty_file_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_empty_file_operations(&fs);
            }

            #[test]
            fn test_large_file_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_large_file_operations(&fs);
            }

            // Directory I/O operations
            #[test]
            fn test_directory_read_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_directory_read_operations(&fs);
            }

            #[test]
            fn test_directory_position_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_directory_position_operations(&fs);
            }

            #[test]
            fn test_directory_rewind() {
                let fs = $fs;
                $crate::file_system::tests::test_directory_rewind(&fs);
            }

            #[test]
            fn test_empty_directory_read() {
                let fs = $fs;
                $crate::file_system::tests::test_empty_directory_read(&fs);
            }

            #[test]
            fn test_directory_entry_metadata() {
                let fs = $fs;
                $crate::file_system::tests::test_directory_entry_metadata(&fs);
            }

            #[test]
            fn test_open_close_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_open_close_operations(&fs);
            }

            // Error handling and edge cases
            #[test]
            fn test_error_handling() {
                let fs = $fs;
                $crate::file_system::tests::test_error_handling(&fs);
            }

            #[test]
            fn test_invalid_paths() {
                let fs = $fs;
                $crate::file_system::tests::test_invalid_paths(&fs);
            }

            #[test]
            fn test_multiple_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_multiple_operations(&fs);
            }

            #[test]
            fn test_file_system_consistency() {
                let fs = $fs;
                $crate::file_system::tests::test_file_system_consistency(&fs);
            }

            #[test]
            fn test_lookup_with_flags() {
                let fs = $fs;
                $crate::file_system::tests::test_lookup_with_flags(&fs);
            }

            #[test]
            fn test_root_directory_operations() {
                let fs = $fs;
                $crate::file_system::tests::test_root_directory_operations(&fs);
            }

            #[test]
            fn test_long_filenames() {
                let fs = $fs;
                $crate::file_system::tests::test_long_filenames(&fs);
            }

            #[test]
            fn test_special_characters_in_names() {
                let fs = $fs;
                $crate::file_system::tests::test_special_characters_in_names(&fs);
            }
        };
    }
    pub use implement_file_system_tests;
}
