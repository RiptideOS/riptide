use alloc::{
    collections::{BTreeMap, VecDeque},
    string::{String, ToString},
    sync::{Arc, Weak},
};
use core::{
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use conquer_once::spin::OnceCell;
use spin::RwLock;

use super::{File, FileDescriptor, FileSystem, FsNode, FsNodeId, path::Path};
use crate::{
    fs::{FileMode, FsNodeKind, MountFlags, registry::find_file_system_type},
    util::defer::defer_handle,
};

#[derive(Debug)]
pub enum IoError {
    /// The requested operation is not implemented by the target file system or
    /// device
    OperationNotSupported,
    /// Some element of a provided path was not found in the file system
    EntryNotFound,
    /// The provided path already exists (for create operations) or is already
    /// occupied by a mounted file system
    AlreadyExists,
    /// The path provided to an operation did not contain a directory where one
    /// was expected (i.e. tried to specify a path segment after the name of an
    /// existing file)
    NotADirectory,
    /// The path provided to an operation did not contain a file where one was
    /// expected (i.e. tried to open a directory as a file)
    NotAFile,
    /// The provided path was not valid (contained invalid characters) or
    /// otherwise could not be parsed
    InvalidPath,
    /// File pointer is not registered in the file table (this file has already
    /// been closed)
    InvalidFile,
    /// The requested operation is not compatible with the mode the target file
    /// was opened with (i.e. trying to write to a file descriptor which was
    /// opened in read mode)
    InvalidMode,
    /// The requested file system type in a mount operation was not found
    FileSystemTypeNotFound,
    /// Only ever returned if a resolution operation is attempted before the
    /// root of the file system has been mounted
    NoRootDirectory,
}

#[derive(Default)]
pub struct VirtualFileSystem {
    /// A list of all the files which are opened by different processes
    files: RwLock<BTreeMap<FileDescriptor, Arc<File>>>,
    /// A table which keeps track of the mount points of file systems
    mount_table: RwLock<BTreeMap<MountId, Arc<VfsMount>>>,
    /// An in-memory cache of directory entries. This maps file names to their
    /// coresponding FsNode objects and is the mechanism used to perform path
    /// walking and lookups. Keeping this cache prevents having to contstantly
    /// query the file system implementation with lookup calls since the
    /// underlying data doesn't change for most file systems.
    directory_cache: RwLock<DirectoryCache>,
}

impl VirtualFileSystem {
    fn new() -> Self {
        Self::default()
    }

    /// Creates an empty ramfs and mounts it as the root directory
    fn create_root(&self) -> Result<Arc<DirectoryEntry>, IoError> {
        assert!(self.directory_cache.read().get_root().is_none());

        let id = self.mount("", "/", Some("ramfs"), MountFlags::READ | MountFlags::WRITE)?;
        let mount = self.get_mount(id).unwrap();

        Ok(mount.root.clone())
    }

    /// Looks up an entry in the cache or attempts to fetch it from the file
    /// system if not found, subsequently inserting it into the cache
    fn get_cached_or_lookup(
        &self,
        parent: &Arc<DirectoryEntry>,
        name: &str,
    ) -> Result<Option<Arc<DirectoryEntry>>, IoError> {
        // check the cache
        if let Some(cached) = self.directory_cache.read().lookup(parent, name) {
            return Ok(Some(cached));
        }

        // check the backing fs of the current top node
        let fs = parent.node.file_system();
        let Some(node) = fs.directory_operations().lookup(parent, name)? else {
            return Ok(None);
        };

        // insert into the cache for future lookups
        let entry = self
            .directory_cache
            .write()
            .insert(Some(parent.clone()), node, name);

        Ok(Some(entry))
    }

    /// Resolves all segments in a path to a directory entry in the VFS,
    /// returning the last entry in the path if all resolutions were successful.
    /// If a path segment cannot be resolved, None is returned. An Err is only
    /// returned if the fs driver encountered an error while performing the io
    /// to lookup a name.
    ///
    /// If a path segment is being resolved for the first time, it is added to
    /// the cache. Subsequent queries while strong references to an entry or one
    /// of its children still exist will result in a faster cache hit. Returned
    /// entries which identify the same entry on disk are guaranteed to have the
    /// same ID for as long as strong referernces to the entry exist in memory.
    /// When reloaded from disk, IDs are regenerated.
    fn resolve_path(&self, path: &str) -> Result<Option<Arc<DirectoryEntry>>, IoError> {
        let path = Path::from_str(path).map_err(|_| IoError::InvalidPath)?;

        if !path.is_absolute() {
            todo!("resolve relative paths ({path:?})");
        }

        let Some(root_directory) = self.directory_cache.read().get_root() else {
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

                    let Some(entry) = self.get_cached_or_lookup(top, name)? else {
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

        let Some(root_directory) = self.directory_cache.read().get_root() else {
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

                        let Some(entry) = self.get_cached_or_lookup(top, name)? else {
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
        // guess based on the magic.
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
            let mut cache = self.directory_cache.write();

            if cache.get_root().is_some() {
                return Err(IoError::AlreadyExists);
            }

            let id = MountId::new();
            let fs = ty.mount(id, source, flags)?;

            let root = cache.insert(None, fs.root_directory(), "/");

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
            // todo: make sure to invalidate directory cache?
            // todo: make sure to lock parent while we do this and then check
            // again

            todo!()
        }
        // Mounting into a non-existent directory.
        else {
            let (parent, name) = self.resolve_path_parent_directory(target)?;

            let _lock = parent.node.structure_lock.lock();

            // FIXME: check that this name is not already mounted in the
            // parent directory
            // FIXME: check that this name is not already taken in the parent
            // dir (after acquiring the lock on the parent)

            let id = MountId::new();
            let fs = ty.mount(id, source, flags)?;

            let mut cache = self.directory_cache.write();
            let root = cache.insert(Some(parent.clone()), fs.root_directory(), name);

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
                let node = fs.directory_operations().create_file(&parent, &file_name)?;

                self.directory_cache
                    .write()
                    .insert(Some(parent), node, file_name)
            }
        } else {
            self.resolve_path(path)?.ok_or(IoError::EntryNotFound)?
        };

        file_entry.node.increment_link_count();
        let error_cleanup = defer_handle!({
            file_entry.node.decrement_link_count();
        });

        let fs = file_entry.node.file_system();
        let file = Arc::new(fs.file_operations().open(file_entry.node.clone(), mode)?);

        let fd = FileDescriptor::new();
        self.files.write().insert(fd, file.clone());

        error_cleanup.cancel();
        Ok(fd)
    }

    /// Flushes a file to disk and removes the descriptor from the table
    pub fn close(&self, fd: FileDescriptor) -> Result<(), IoError> {
        let file = self.get_file(fd)?;

        let fs = file.file_system();
        fs.file_operations().flush(&file)?;

        self.files.write().remove(&fd);
        file.node.decrement_link_count();

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
    pub fn read_directory(&self, path: &str) -> Result<DirectoryIterationContext, IoError> {
        let directory = self.resolve_path(path)?.ok_or(IoError::EntryNotFound)?;

        // Dont allow modification to this directory while we are iterating it
        let _guard = directory.node.structure_lock.lock();

        if !directory.node.is_directory() {
            return Err(IoError::NotADirectory);
        }

        let mut ctx = DirectoryIterationContext::new();

        // Default readdir for this file system
        let fs = directory.node.file_system();
        fs.directory_operations()
            .read_directory(&mut ctx, &directory)?;

        // Any VFS mounts whose root directory is within this directory should
        // also be added to the result
        for mnt in self.mount_table.read().values() {
            let Some(parent) = &mnt.root.parent else {
                continue;
            };

            if *parent == directory {
                ctx.insert(&mnt.root.name, mnt.root.node.id, mnt.root.node.kind);
            }
        }

        Ok(ctx)
    }

    pub fn create_directory(&self, path: &str) -> Result<Arc<DirectoryEntry>, IoError> {
        if self.resolve_path(path)?.is_some() {
            return Err(IoError::AlreadyExists);
        }

        let (parent, dir_name) = self.resolve_path_parent_directory(path)?;

        // Lock the parent to make sure that we dont try to create or delete
        // other entries concurrently
        let _guard = parent.node.structure_lock.lock();

        let fs = parent.node.file_system();
        let node = fs
            .directory_operations()
            .create_directory(&parent, &dir_name)?;

        let entry = self
            .directory_cache
            .write()
            .insert(Some(parent.clone()), node, dir_name);

        Ok(entry)
    }

    pub fn stat(&self, path: &str) -> Result<Arc<DirectoryEntry>, IoError> {
        self.resolve_path(path)?.ok_or(IoError::EntryNotFound)
    }

    /// Locks the directory cache and performs a prune operation to free unused
    /// memory. Should really only be called while the system is under high
    /// memory pressure.
    pub fn prune_directory_cache(&self) {
        let mut cache = self.directory_cache.write();
        cache.prune();
    }
}

pub struct VfsMount {
    /// Uniquely identifies this mount (fs instance) within the VFS. Regenerated
    /// on each successful mount invocation.
    id: MountId,
    /// A reference to the root directory which this file system is mounted on.
    /// Keeping a strong reference here prevents the entry from ever being
    /// evicted from the directory cache
    root: Arc<DirectoryEntry>,
    /// A reference to the instance of the mounted file system
    pub file_system: Arc<dyn FileSystem>,
    // TODO: do we need a counter of references to this mount so we know if we
    // can safely unmount it?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MountId(u64);

impl MountId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Entries can only be created by the DirectoryCache. This ensures that no more
/// than one DirectoryEntry object with the same parent and name is allocated at
/// once. Without this constraint, maintaining consistency when moving and
/// renaming would be impossible.
#[derive(Debug)]
pub struct DirectoryEntry {
    /// Uniquely identifies this directory entry while there is a strong
    /// reference to it. Once no strong references exist, looking up the entry
    /// again will perform a full fs lookup and generate a new id. This ID
    /// should only be used in the context of the cache and not by consumers of
    /// this type. Use [`Arc`] or [`Weak`] for those purposes which also encode
    /// ownership semantics.
    id: DirectoryEntryId,

    pub name: Arc<str>,
    pub node: Arc<FsNode>,

    // Entires always retain a strong reference to their parent to make sure
    // their parent is never evicted from the directory cache. Since the
    // parent's id is used as the cache key, there is no way to find this node
    // without doing a full fs lookup if the parent is dropped.
    pub parent: Option<Arc<DirectoryEntry>>,
    /// Children retain a weak reference to alow them to be garbage collected
    /// when there is high memory pressure.
    pub children: RwLock<BTreeMap<Arc<str>, Weak<DirectoryEntry>>>,
}

impl PartialEq for DirectoryEntry {
    fn eq(&self, other: &Self) -> bool {
        // NOTE: the directory cache ensures that if an entry exists in the
        // cache (and is accesible), its ID is guaranteed to uniquely identify
        // that entry within the file system. For this reason, we can be
        // confident that if the IDs match then they are the same entry.
        self.id == other.id
    }
}

impl DirectoryEntry {
    /// Removes entries in the child cache which have already been garbage
    /// collected
    fn prune_children(&self) {
        let mut children = self.children.write();
        children.retain(|_, w| w.strong_count() > 0);
    }
}

/// A guaranteed globally unique key which identifies a particular directory
/// entry. For as long as there exist any strong references to that entry, it is
/// guaranteed that no other IDs will exist for that parent and child name pair
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DirectoryEntryId(u64);

impl DirectoryEntryId {
    pub const NULL: Self = Self(0);

    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// A cache for resolved directory entries. All directory entries with a live
/// reference count are guaranteed to live in this table. Once no longer in use,
/// entries may be evicted at any time on an LRU basis. This type is used
/// internally by the VFS.
#[derive(Debug, Default)]
struct DirectoryCache {
    table: BTreeMap<DirectoryCacheKey, Weak<DirectoryEntry>>,
}

/// A combination of the parent ID and child name, used to index the directory
/// cache.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DirectoryCacheKey(DirectoryEntryId, Arc<str>);

impl DirectoryCache {
    /// Gets the root directory entry if it has been inserted into the cache
    fn get_root(&self) -> Option<Arc<DirectoryEntry>> {
        let key = DirectoryCacheKey(DirectoryEntryId::NULL, "/".into());
        self.table.get(&key).and_then(|w| w.upgrade())
    }

    /// Creates an entry in the cache and returns a strong reference
    fn insert(
        &mut self,
        parent: Option<Arc<DirectoryEntry>>,
        node: Arc<FsNode>,
        name: impl Into<Arc<str>>,
    ) -> Arc<DirectoryEntry> {
        let name = name.into();

        assert!(
            parent.is_some() || name.as_ref() == "/",
            "only the root entry is allowed to not have a parent"
        );

        if let Some(parent) = &parent {
            // FIXME: should we just return the existing entry here instead of
            // panicking?
            assert!(
                self.lookup(parent, &name).is_none(),
                "attempted to re-insert existing entry"
            );
        }

        let entry = Arc::new(DirectoryEntry {
            id: DirectoryEntryId::new(),
            name,
            node,
            parent: parent.clone(),
            children: Default::default(),
        });

        if let Some(parent) = parent {
            parent
                .children
                .write()
                .insert(entry.name.clone(), Arc::downgrade(&entry));
        }

        let key = DirectoryCacheKey(
            entry
                .parent
                .as_ref()
                .map(|p| p.id)
                .unwrap_or(DirectoryEntryId::NULL),
            entry.name.clone(),
        );
        self.table.insert(key, Arc::downgrade(&entry));

        entry
    }

    /// Gets a key from the cache if it exists. This does not perform any file
    /// system operations or name resolution.
    fn lookup(&self, parent: &Arc<DirectoryEntry>, name: &str) -> Option<Arc<DirectoryEntry>> {
        let key = DirectoryCacheKey(parent.id, name.into());
        self.table.get(&key).and_then(|w| w.upgrade())
    }

    /// Removes any entries from the table which havve a reference count of 0
    fn prune(&mut self) {
        self.table.retain(|_, w| w.strong_count() > 0);

        for w in self.table.values_mut() {
            if let Some(e) = w.upgrade() {
                e.prune_children();
            }
        }
    }
}

pub struct DirectoryIterationContext {
    table: BTreeMap<Arc<str>, DirectoryIterationEntry>,
}

pub struct DirectoryIterationEntry {
    pub name: Arc<str>,
    pub id: FsNodeId,
    pub kind: FsNodeKind,
    _private: (),
}

impl From<&DirectoryEntry> for DirectoryIterationEntry {
    fn from(value: &DirectoryEntry) -> Self {
        Self {
            name: value.name.clone(),
            id: value.node.id,
            kind: value.node.kind,
            _private: (),
        }
    }
}

impl DirectoryIterationContext {
    fn new() -> Self {
        Self {
            table: Default::default(),
        }
    }

    pub fn insert(&mut self, name: &str, id: FsNodeId, kind: FsNodeKind) {
        let name: Arc<str> = name.into();

        self.table.insert(
            name.clone(),
            DirectoryIterationEntry {
                name,
                id,
                kind,
                _private: (),
            },
        );
    }
}

impl IntoIterator for DirectoryIterationContext {
    type Item = DirectoryIterationEntry;
    type IntoIter = alloc::collections::btree_map::IntoValues<Arc<str>, DirectoryIterationEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.table.into_values()
    }
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
