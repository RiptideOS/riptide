use alloc::{
    collections::{BTreeMap, VecDeque},
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::{
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use conquer_once::spin::OnceCell;
use spin::RwLock;

use super::{File, FileDescriptor, FileSystem, FsNode, path::Path};
use crate::fs::{FileMode, FsNodeKind, MountFlags, registry::find_file_system_type};

#[derive(Default)]
pub struct VirtualFileSystem {
    /// A list of all the files which are opened by different processes
    files: RwLock<BTreeMap<FileDescriptor, Arc<File>>>,
    /// A table which keeps track of the mount points of file systems
    mount_table: RwLock<BTreeMap<MountId, Arc<VfsMount>>>,

    root_directory: RwLock<Option<Arc<DirectoryEntry>>>,
}

impl VirtualFileSystem {
    fn new() -> Self {
        Self::default()
    }

    /// Creates an empty ramfs and mounts it as the root directory
    fn create_root(&self) -> Result<Arc<DirectoryEntry>, IoError> {
        assert!(self.root_directory.read().is_none());

        let id = self.mount("", "/", Some("ramfs"), MountFlags::READ | MountFlags::WRITE)?;
        let mount = self.get_mount(id).unwrap();

        Ok(mount.root.clone())
    }

    /// Resolves all segments in a path to a directory entry in the VFS,
    /// returning the last entry in the path if all resolutions were successful.
    /// If a path segment cannot be resolved, None is returned. An Err is only
    /// returned if the fs driver encountered an error while performing the io
    /// to lookup a name.
    fn resolve_path(&self, path: &str) -> Result<Option<Arc<DirectoryEntry>>, IoError> {
        let path = Path::from_str(path).map_err(|_| IoError::InvalidPath)?;

        if !path.is_absolute() {
            todo!("resolve relative paths");
        }

        let Some(root_directory) = self.root_directory.read().clone() else {
            return Err(IoError::NoRootDirectory);
        };

        let mut stack = VecDeque::new();
        stack.push_back(root_directory.clone());

        // we know the first segment is the root so we can skip it
        'segments: for segment in path.segments().skip(1) {
            let top = stack.back().expect("root should always exist");

            // Every additional segment we add requires that the previous
            // segment be a directory
            if !top.node.is_directory() {
                return Err(IoError::NotADirectory);
            }

            match segment {
                "." => {
                    // single dots are redundant in absolute paths
                    continue;
                }
                ".." => {
                    // If only the root exists, the POSIX behavior is to ignore
                    // any additional double dots
                    if stack.len() == 1 {
                        continue;
                    }

                    stack.pop_back();
                }
                name => {
                    // lookup this name in the directory which is on the top of
                    // the resolution stack

                    // check if the top dir is the parent of any mounts in the
                    // mount table. if it is, check those mounts before querying
                    // the original fs
                    for mnt in self.mount_table.read().values() {
                        if mnt.root.parent.as_ref().is_some_and(|p| p == top)
                            && *mnt.root.name == *name
                        {
                            stack.push_back(mnt.root.clone());
                            continue 'segments;
                        }
                    }

                    // check the backing fs of the current top node
                    let fs = top.node.file_system();
                    let Some(entry) = fs.directory_operations().lookup(top.clone(), name)? else {
                        return Ok(None);
                    };

                    stack.push_back(entry);
                }
            }
        }

        Ok(Some(stack.pop_back().unwrap()))
    }

    /// Resolves all segments in a path to a directory entry in the VFS,
    /// excluding the last segment which is not a "." or "..". All resolved
    /// segments must be directory nodes.
    fn resolve_path_parent_directory(
        &self,
        path: &str,
    ) -> Result<(Arc<DirectoryEntry>, String), IoError> {
        let path = Path::from_str(path).map_err(|_| IoError::InvalidPath)?;
        if !path.is_absolute() {
            todo!("canonicalize relative paths");
        }

        let Some(root_directory) = self.root_directory.read().clone() else {
            return Err(IoError::NoRootDirectory);
        };

        let mut stack = VecDeque::new();
        stack.push_back(root_directory.clone());

        // number of segments after the root
        let count = path.segments().count() - 1;

        // Must have at least one segment after the root directory
        if count == 0 {
            return Err(IoError::InvalidPath);
        }

        // we know the first segment is the root so we can skip it
        for (i, segment) in path.segments().skip(1).enumerate() {
            match segment {
                "." => {
                    todo!()
                }
                ".." => {
                    todo!()
                }
                name => {
                    // FIXME: this doesnt handle "." or ".." at the end

                    if i != count - 1 {
                        // lookup this name in the directory which is on the top of
                        // the resolution stack

                        let top = stack.back().expect("root should always exist");
                        let fs = top.node.file_system();

                        let Some(entry) = fs.directory_operations().lookup(top.clone(), name)?
                        else {
                            return Err(IoError::EntryNotFound);
                        };

                        if !entry.node.is_directory() {
                            return Err(IoError::NotADirectory);
                        }

                        stack.push_back(entry);
                    } else {
                        return Ok((stack.pop_back().unwrap(), name.to_string()));
                    }
                }
            }
        }

        unreachable!();
    }

    /// Looks up a mount in the global VFS mount table
    pub(super) fn get_mount(&self, id: MountId) -> Option<Arc<VfsMount>> {
        self.mount_table.read().get(&id).cloned()
    }

    pub fn get_mount_root(&self, id: MountId) -> Option<Arc<DirectoryEntry>> {
        self.get_mount(id).map(|m| m.root.clone())
    }

    /// Mounts the given file system in the specified directory. The backing FS
    /// can be a block device or a regular file.
    pub fn mount(
        &self,
        source: &str,
        target: &str,
        kind: Option<&str>,
        flags: MountFlags,
    ) -> Result<MountId, IoError> {
        // If a desired type was specified, use that. Otherwise we will try to
        // guess based on the magic.ç
        let fs_type = match kind {
            Some(k) => Some(find_file_system_type(k).ok_or(IoError::FileSystemTypeNotFound)?),
            None => None,
        };

        let Some(ty) = fs_type else {
            todo!("handle fs type detection based on longest matching sequence of magic bytes")
        };

        if ty.metadata().name != "ramfs" && ty.metadata().name != "devfs" {
            todo!("we can only mount virtual file systems for now (no block devices)")
        }

        // There is a special case here if we are mounting the root of the
        // entire VFS because there is additional state we need to initialize.
        let mount = if target == "/" {
            let mut vfs_root = self.root_directory.write();

            if vfs_root.is_some() {
                return Err(IoError::RootAlreadyExists);
            }

            let id = MountId::new();
            let fs = ty.mount(id, source, flags)?;

            let root = Arc::new(DirectoryEntry {
                name: "/".into(),
                node: fs.root_directory(),
                parent: None,
            });

            *vfs_root = Some(root.clone());

            VfsMount {
                id,
                root,
                file_system: fs,
            }
        }
        // Mounting over an existing directory
        else if let Some(_target) = self.resolve_path(target)? {
            // let id = MountId::new();
            // let fs = ty.mount(id, source, flags)?;

            // todo: check if is dir

            todo!()
        }
        // Mounting into a non-existent directory.
        else {
            let (parent, name) = self.resolve_path_parent_directory(target)?;

            // FIXME: check that this name is not already mounted in the
            // parent directory

            let id = MountId::new();
            let fs = ty.mount(id, source, flags)?;

            let root = Arc::new(DirectoryEntry {
                name: name.into(),
                node: fs.root_directory(),
                parent: Some(parent),
            });

            VfsMount {
                id,
                root,
                file_system: fs,
            }
        };

        let id = mount.id;
        self.mount_table.write().insert(id, Arc::new(mount));

        Ok(id)
    }

    fn get_file(&self, fd: FileDescriptor) -> Result<Arc<File>, IoError> {
        self.files
            .read()
            .get(&fd)
            .ok_or(IoError::InvalidFile)
            .cloned()
    }

    /// Opens the given path as a file or creates one if the file does not
    /// already exist
    pub fn open(&self, path: &str, mode: FileMode) -> Result<FileDescriptor, IoError> {
        // resolve the file entry or create a new one in the parent directory if
        // we are opening in a writing mode
        let file_entry = if mode.is_mutating() {
            // return the file if it exists, or try to create it as long as the
            // parent directory exists
            if let Some(entry) = self.resolve_path(path)? {
                if entry.node.is_directory() {
                    return Err(IoError::NotAFile);
                }

                entry
            } else {
                let (parent, file_name) = self.resolve_path_parent_directory(path)?;

                let fs = parent.node.file_system();
                fs.directory_operations()
                    .create_file(parent.clone(), &file_name)?
            }
        } else {
            self.resolve_path(path)?.ok_or(IoError::EntryNotFound)?
        };

        let fs = file_entry.node.file_system();
        let file = Arc::new(fs.file_operations().open(file_entry.node.clone(), mode)?);

        let fd = FileDescriptor::new();
        self.files.write().insert(fd, file.clone());

        Ok(fd)
    }

    /// Flushes a file to disk and removes the descriptor from the table
    pub fn close(&self, fd: FileDescriptor) -> Result<(), IoError> {
        let file = self.get_file(fd)?;

        let fs = file.file_system();
        fs.file_operations().flush(&file)?;

        self.files.write().remove(&fd);

        Ok(())
    }

    /// Reads from the file into the buffer at the current file offset. Returns
    /// the number of bytes read.
    pub fn read(&self, fd: FileDescriptor, buffer: &mut [u8]) -> Result<usize, IoError> {
        let file = self.get_file(fd)?;
        assert_ne!(file.node.kind, FsNodeKind::Directory);

        if file.mode != FileMode::Read {
            return Err(IoError::InvalidMode);
        }

        // FIXME: check that buffer is smaller than max read size
        // FIXME: update file access time

        let fs = file.file_system();

        /* Read and update the current offset if successful */

        let mut offset = file.position.lock();

        let n = fs.file_operations().read(&file, *offset, buffer)?;
        *offset += n;

        Ok(n)
    }

    /// Write to the file from the buffer at the current file offset. Returns
    /// the number of bytes written.
    pub fn write(&self, fd: FileDescriptor, buffer: &[u8]) -> Result<usize, IoError> {
        let file = self.get_file(fd)?;
        assert_ne!(file.node.kind, FsNodeKind::Directory);

        if file.mode != FileMode::Write {
            return Err(IoError::InvalidMode);
        }

        // FIXME: check that buffer is smaller than max write size
        // FIXME: update file modify time

        let fs = file.file_system();

        /* Write and update the current offset if successful */

        let mut offset = file.position.lock();

        let n = fs.file_operations().write(&file, *offset, buffer)?;
        *offset += n;

        Ok(n)
    }

    /// Lists the contents of a directory in the virtual file system. Uses the
    /// FsNode assiciated with the provided path as well as entries from the
    /// mount table.
    pub fn read_directory(&self, path: &str) -> Result<Vec<Arc<DirectoryEntry>>, IoError> {
        let directory = self.resolve_path(path)?.ok_or(IoError::EntryNotFound)?;
        if !directory.node.is_directory() {
            return Err(IoError::NotADirectory);
        }

        // we need to collect results in a map because mounts may be mounted on
        // top of existing directory entries
        let mut res = BTreeMap::new();

        // Default readdir for this file system
        let fs = directory.node.file_system();
        res.extend(
            &mut fs
                .directory_operations()
                .read_directory(directory.clone())?
                .into_iter()
                .map(|d| (d.name.clone(), d)),
        );

        // Any VFS mounts whose root directory is within this directory should
        // also be added to the result
        for mnt in self.mount_table.read().values() {
            let Some(parent) = &mnt.root.parent else {
                continue;
            };

            if parent == &directory {
                res.insert(mnt.root.name.clone(), mnt.root.clone());
            }
        }

        Ok(res.into_values().collect())
    }

    pub fn stat(&self, path: &str) -> Result<Arc<DirectoryEntry>, IoError> {
        self.resolve_path(path)?.ok_or(IoError::EntryNotFound)
    }
}

#[derive(Debug)]
pub enum IoError {
    OperationNotSupported,
    InvalidMode,
    FileSystemTypeNotFound,
    EntryNotFound,
    NotADirectory,
    NotAFile,
    RootAlreadyExists,
    NoRootDirectory,
    InvalidPath,
    /// File pointer is not registered in the file table (this file has already
    /// been closed)
    InvalidFile,
}

#[derive(Debug, PartialEq)]
pub struct DirectoryEntry {
    pub name: Arc<str>,
    pub node: Arc<FsNode>,
    pub parent: Option<Arc<DirectoryEntry>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MountId(u64);

impl MountId {
    pub const NULL: Self = Self(0);

    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct VfsMount {
    id: MountId,
    /// A reference to the root directory which this file system is mounted on
    root: Arc<DirectoryEntry>,
    /// A reference to the instance of the mounted file system
    pub file_system: Arc<dyn FileSystem>,
    // TODO: do we need a counter of references to this mount so we know if we
    // can safely unmount it?
}

static VFS: OnceCell<VirtualFileSystem> = OnceCell::uninit();

/// Allocates memory for the VFS and mounts the init ram fs
pub fn init() {
    let vfs = VFS.get_or_init(VirtualFileSystem::new);
    vfs.create_root().expect("Failed to create root directory");

    vfs.mount(
        "",
        "/dev",
        Some("devfs"),
        MountFlags::READ | MountFlags::WRITE,
    )
    .expect("Failed to mount devfs");

    let f = vfs
        .open("/test.txt", FileMode::Write)
        .expect("Failed to open file for writing");

    vfs.write(f, b"Hello, world!")
        .expect("Failed to write to file");

    vfs.close(f).expect("Failed to close file");
}

pub fn get() -> &'static VirtualFileSystem {
    VFS.get().expect("VFS not yet initialized")
}
