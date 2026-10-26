use super::{FilePickerMode, FilePickerStatus};
use alloc::{string::String, vec::Vec};
use file_system::{AccessFlags, Entry, Kind, Path};
use task::TaskIdentifier;
use virtual_file_system::{
    Error, Result, SynchronousDirectory, SynchronousFile, VirtualFileSystem,
};

pub const PAGE_SIZE: usize = 16;
pub const MAX_PATH: usize = 1023;

pub fn normalize(path: &str) -> Result<String> {
    if !path.starts_with('/') || path.len() > MAX_PATH || path.as_bytes().contains(&0) {
        return Err(Error::InvalidPath);
    }
    let mut result = String::from("/");
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if let Some(index) = result.rfind('/') {
                    result.truncate(index.max(1));
                }
            }
            _ => {
                if result.len() > 1 {
                    result.push('/');
                }
                result.push_str(part);
            }
        }
    }
    Ok(result)
}
pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(index) => &path[..index],
    }
}
pub struct Page {
    pub entries: Vec<Entry>,
    pub more: bool,
}
pub trait Browser {
    fn kind(&self, task: TaskIdentifier, path: &str) -> Result<Kind>;
    fn file_access(&self, task: TaskIdentifier, path: &str, write: bool) -> Result<()>;
    fn page(&self, task: TaskIdentifier, path: &str, offset: usize) -> Result<Page>;
}
impl Browser for VirtualFileSystem {
    fn file_access(&self, task: TaskIdentifier, path: &str, write: bool) -> Result<()> {
        // No create/truncate flags: validation must preserve existing contents.
        SynchronousFile::open(
            self,
            task,
            Path::from_str(path),
            if write {
                AccessFlags::Write
            } else {
                AccessFlags::Read
            }
            .into(),
        )?
        .close(self)
    }
    fn kind(&self, task: TaskIdentifier, path: &str) -> Result<Kind> {
        Ok(task::block_on(self.get_statistics_for_task(task, &Path::from_str(path)))?.kind)
    }
    fn page(&self, task: TaskIdentifier, path: &str, offset: usize) -> Result<Page> {
        let mut directory = SynchronousDirectory::open(self, task, Path::from_str(path))?;
        let mut entries = Vec::with_capacity(PAGE_SIZE);
        let mut skipped = 0;
        while let Some(entry) = directory.read()? {
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            if entry.name.len() > MAX_PATH
                || entry.name.contains('/')
                || entry.name.as_bytes().contains(&0)
            {
                continue;
            }
            if skipped < offset {
                skipped += 1;
                continue;
            }
            if entries.len() == PAGE_SIZE {
                return Ok(Page {
                    entries,
                    more: true,
                });
            }
            entries.push(entry);
        }
        Ok(Page {
            entries,
            more: false,
        })
    }
}
pub struct Model {
    pub caller: TaskIdentifier,
    pub mode: FilePickerMode,
    pub status: FilePickerStatus,
    pub directory: String,
    pub offset: usize,
    pub page: Page,
    pub selected: Option<String>,
}
impl Model {
    pub fn new(caller: TaskIdentifier, mode: FilePickerMode) -> Self {
        Self {
            caller,
            mode,
            status: FilePickerStatus::Pending,
            directory: String::from("/"),
            offset: 0,
            page: Page {
                entries: Vec::new(),
                more: false,
            },
            selected: None,
        }
    }
    pub fn navigate(&mut self, browser: &impl Browser, path: &str, offset: usize) -> Result<()> {
        if self.status != FilePickerStatus::Pending {
            return Err(Error::InvalidParameter);
        }
        let path = normalize(path)?;
        let page = browser.page(self.caller, &path, offset)?;
        self.directory = path;
        self.offset = offset;
        self.page = page;
        Ok(())
    }
    pub fn confirm(&mut self, browser: &impl Browser, path: &str) -> Result<()> {
        if self.status != FilePickerStatus::Pending {
            return Err(Error::InvalidParameter);
        }
        let path = normalize(path)?;
        match (self.mode, browser.kind(self.caller, &path)) {
            (FilePickerMode::Directory, Ok(Kind::Directory)) => {
                let _ = browser.page(self.caller, &path, 0)?;
            }
            (FilePickerMode::OpenFile | FilePickerMode::SaveFile, Ok(Kind::File)) => {
                browser.file_access(self.caller, &path, self.mode == FilePickerMode::SaveFile)?;
            }
            (FilePickerMode::SaveFile, Err(Error::FileSystem(file_system::Error::NotFound))) => {
                if browser.kind(self.caller, parent(&path))? != Kind::Directory {
                    return Err(Error::NotADirectory);
                }
                // Reading the parent verifies traversal/read permission without creating a target.
                let _ = browser.page(self.caller, parent(&path), 0)?;
            }
            (_, Err(error)) => return Err(error),
            (FilePickerMode::Directory, _) => return Err(Error::NotADirectory),
            _ => return Err(Error::NotAFile),
        }
        self.selected = Some(path);
        self.status = FilePickerStatus::Selected;
        Ok(())
    }
    pub fn cancel(&mut self) {
        if self.status == FilePickerStatus::Pending {
            self.status = FilePickerStatus::Cancelled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_absolute_paths_and_enforces_byte_bounds() {
        assert_eq!(normalize("//a/./b/../c/").unwrap(), "/a/c");
        assert_eq!(normalize("/../../a").unwrap(), "/a");
        assert!(normalize("relative").is_err());
        assert!(normalize("/a\0b").is_err());
        assert!(normalize(&alloc::format!("/{}", "a".repeat(1023))).is_err());
        assert_eq!(
            normalize(&alloc::format!("/{}", "a".repeat(1022)))
                .unwrap()
                .len(),
            1023
        );
    }
}

#[cfg(test)]
mod behavior_tests {
    use super::*;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    struct Backend {
        calls: RefCell<Vec<TaskIdentifier>>,
    }
    impl Browser for Backend {
        fn file_access(&self, task: TaskIdentifier, _: &str, _: bool) -> Result<()> {
            self.calls.borrow_mut().push(task);
            Ok(())
        }
        fn kind(&self, task: TaskIdentifier, path: &str) -> Result<Kind> {
            self.calls.borrow_mut().push(task);
            match path {
                "/" => Ok(Kind::Directory),
                "/file" => Ok(Kind::File),
                "/missing" => Err(Error::FileSystem(file_system::Error::NotFound)),
                _ => Err(Error::PermissionDenied),
            }
        }
        fn page(&self, task: TaskIdentifier, path: &str, offset: usize) -> Result<Page> {
            self.calls.borrow_mut().push(task);
            if path != "/" {
                return Err(Error::PermissionDenied);
            }
            let mut entries = Vec::new();
            for i in offset..core::cmp::min(offset + PAGE_SIZE, 33) {
                entries.push(Entry::new(
                    i as _,
                    alloc::format!("entry{i}"),
                    Kind::File,
                    0,
                ));
            }
            Ok(Page {
                entries,
                more: offset + PAGE_SIZE < 33,
            })
        }
    }
    #[test]
    fn pages_and_errors_preserve_directory_and_caller_identity() {
        let backend = Backend {
            calls: RefCell::new(Vec::new()),
        };
        let caller = TaskIdentifier::new(42);
        let mut model = Model::new(caller, FilePickerMode::OpenFile);
        model.navigate(&backend, "/", 0).unwrap();
        assert_eq!(model.page.entries.len(), 16);
        assert!(model.page.more);
        model.navigate(&backend, "/", 16).unwrap();
        assert_eq!(model.page.entries[0].name, "entry16");
        model.navigate(&backend, "/", 32).unwrap();
        assert_eq!(model.page.entries.len(), 1);
        assert!(!model.page.more);
        assert_eq!(
            model.navigate(&backend, "/private", 0),
            Err(Error::PermissionDenied)
        );
        assert_eq!(model.directory, "/");
        assert_eq!(model.offset, 32);
        assert!(backend.calls.borrow().iter().all(|id| *id == caller));
    }
    #[test]
    fn modes_cancel_and_save_do_not_create_files() {
        let backend = Backend {
            calls: RefCell::new(Vec::new()),
        };
        let mut model = Model::new(TaskIdentifier::new(7), FilePickerMode::OpenFile);
        assert!(model.confirm(&backend, "/").is_err());
        assert!(model.confirm(&backend, "/missing").is_err());
        model.confirm(&backend, "/file").unwrap();
        assert_eq!(model.status, FilePickerStatus::Selected);
        assert_eq!(model.selected.as_deref(), Some("/file"));
        let mut model = Model::new(TaskIdentifier::new(7), FilePickerMode::Directory);
        assert!(model.confirm(&backend, "/file").is_err());
        model.confirm(&backend, "/").unwrap();
        let mut model = Model::new(TaskIdentifier::new(7), FilePickerMode::SaveFile);
        model.confirm(&backend, "/missing").unwrap();
        assert_eq!(model.selected.as_deref(), Some("/missing"));
        let mut model = Model::new(TaskIdentifier::new(7), FilePickerMode::OpenFile);
        model.cancel();
        assert_eq!(model.status, FilePickerStatus::Cancelled);
        assert!(model.selected.is_none());
        assert!(model.confirm(&backend, "/file").is_err());
    }
}
