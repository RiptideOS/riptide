use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
    any::Any,
    fmt::Display,
    sync::atomic::{AtomicU64, Ordering},
};

use spin::Mutex;
use vfs::{DirectoryEntry, IoError, MountId};

use crate::drivers;

pub mod path;
pub mod registry;
pub mod vfs;

/// Represents a driver for a particular file system. Once mounted, the instance
/// of the file system is represented by the [`FileSystem`] trait
pub trait FileSystemType: Send + Sync {
    /// Returns metadata about the file system type like the name and
    /// characteristics
    fn metadata(&self) -> &FileSystemTypeMetadata;

    /// Create a new instance of this file system from from the given device
    fn mount(
        self: Arc<Self>,
        id: MountId,
        source: &str,
        flags: MountFlags,
    ) -> Result<Arc<dyn FileSystem>, IoError>;

    /// Attempt to unmount the given file system instance
    ///
    /// Should only ever be called after all open files referencing this
    /// instance have been closed and written to disk
    fn unmount(self: Arc<Self>, instance: Arc<dyn FileSystem>);
}

pub struct FileSystemTypeMetadata {
    /// Name which identifies the file system type (should be unique)
    pub name: &'static str,
    /// Magic bytes which can be used to identify a particular file system type
    /// when probing a disk
    pub magic: &'static [u8],
}

/// Represents a driver for an instance of a particular file system after it has
/// been mounted in the VFS tree. Analigous to the notion of a `super_block` in
/// Linux.
pub trait FileSystem: Send + Sync {
    /// Returns metadata about the file system
    fn metadata(&self) -> &FileSystemMetadata;

    /// Returns the directory entry which represents the root of this file
    /// system instance (the mount point)
    fn root_directory(&self) -> Arc<FsNode>;

    /// Returns a pointer to a trait object which handles operations on FsNodes
    /// (usually self)
    fn node_operations(&self) -> &dyn FsNodeOperations;

    /// Returns a pointer to a trait object which handles operations on File
    /// objects (usually self)
    fn file_operations(&self) -> &dyn FileOperations;

    /// Returns a pointer to a trait object which handles operations on
    /// Directory objects (usually self)
    fn directory_operations(&self) -> &dyn DirectoryOperations;
}

pub struct FileSystemMetadata {
    /// The ID of the physical device which backs this file system instance.
    /// Will be None if this file system does not live on a physical device.
    pub device: Option<u64>,
    /// Flags which this file system has been mounted with (i.e. read/write
    /// permissions)
    pub mount_flags: MountFlags,
    /// The block size in bytes
    pub block_size: usize,
    /// The maximum file size which this file system supports
    pub max_file_size: usize,
    /// A pointer to the file system type driver
    pub file_system_type: Arc<dyn FileSystemType>,
}

bitflags::bitflags! {
    pub struct MountFlags: u32 {
        const READ = 0b00000001;
        const WRITE = 0b00000010;
    }
}

pub trait FsNodeOperations {
    /// Write a file system node back to the disk (after an operation has been
    /// performed on it)
    fn write_node(&self, node: &FsNode) -> Result<(), ()>;

    /// Removes a file system node from the disk after a delete operation
    fn evict_node(&self, node: &FsNode) -> Result<(), ()>;
}

/// A trait representing all operations which the VFS performs on files that can
/// be overriden by file system drivers
#[allow(unused)]
pub trait FileOperations: Send + Sync {
    /// Hook for files being opened. Can be used to initialize the private data
    /// field.
    fn open(&self, node: Arc<FsNode>, mode: FileMode) -> Result<File, IoError> {
        Ok(File::new(node, mode))
    }

    /// Hook for files being closed. This is a good palce to handle any tear
    /// down for the private data field or perform any other side effects
    /// associated with closing files.
    fn flush(&self, file: &File) -> Result<(), IoError> {
        Ok(())
    }

    /// Called when a a file cursor wants to be repositioned
    fn seek(&self, file: &File, offset: usize) -> Result<usize, IoError> {
        Err(IoError::OperationNotSupported)
    }

    /// Called when data needs to be read from a file. Reads data from the
    /// provided offset into the buffer and returns the number of bytes read.
    fn read(&self, file: &File, offset: usize, buffer: &mut [u8]) -> Result<usize, IoError> {
        Err(IoError::OperationNotSupported)
    }

    /// Called when data needs to be written to file. Writes data at the
    /// provided offset from the buffer and returns the number of bytes written.
    fn write(&self, file: &File, offset: usize, buffer: &[u8]) -> Result<usize, IoError> {
        Err(IoError::OperationNotSupported)
    }
}

/// A trait representing all operations which the VFS performs on directories
/// and can be overriden by file system drivers
pub trait DirectoryOperations: Send + Sync {
    /// Creates a new file on disk and allocates a new FsNodeId
    fn create_file(
        &self,
        _directory: Arc<DirectoryEntry>,
        _name: &str,
    ) -> Result<Arc<DirectoryEntry>, IoError> {
        Err(IoError::OperationNotSupported)
    }

    /// Creates a new directory on disk and allocates a new FsNodeId
    fn create_directory(
        &self,
        _directory: Arc<DirectoryEntry>,
        _name: &str,
    ) -> Result<Arc<DirectoryEntry>, IoError> {
        Err(IoError::OperationNotSupported)
    }

    /// Removes a file in this directory from disk
    fn remove_file(&self) -> Result<Arc<FsNode>, IoError> {
        Err(IoError::OperationNotSupported)
    }

    /// Removes an empty child directory from disk
    fn remove_directory(&self) -> Result<Arc<FsNode>, IoError> {
        Err(IoError::OperationNotSupported)
    }

    /// Looks up an FsNode by name in this directory
    fn lookup(
        &self,
        entry: Arc<DirectoryEntry>,
        name: &str,
    ) -> Result<Option<Arc<DirectoryEntry>>, IoError>;

    /// Iterates all the entries in this directory
    ///
    /// FIXME: use an iterator and/or cursor position to limit the number of
    /// responses for large directories
    fn read_directory(
        &self,
        entry: Arc<DirectoryEntry>,
    ) -> Result<Vec<Arc<DirectoryEntry>>, IoError>;
}

macro_rules! impl_fs_ops_for_self {
    () => {
        fn node_operations(&self) -> &dyn $crate::fs::FsNodeOperations {
            self
        }

        fn file_operations(&self) -> &dyn $crate::fs::FileOperations {
            self
        }

        fn directory_operations(&self) -> &dyn $crate::fs::DirectoryOperations {
            self
        }
    };
}
pub(crate) use impl_fs_ops_for_self;

/// A generic, type erased VFS node. The combination of the id and mount_id
/// uniquely identify this node within the VFS.
///
/// FIXME: centralize the creation of these objects so that we can keep track of
/// them without creating duplicates
#[derive(Debug)]
pub struct FsNode {
    /// The unique identifier which is used to index the backing file system
    pub id: FsNodeId,
    /// A reference to the backing file system that contains this node
    pub mount_id: MountId,
    /// The type of node and a pointer to the corresponding trait object which
    /// implements it's operations
    pub kind: FsNodeKind,
    /// Marker for the VFS to keep track of whether this node needs to be
    /// written to disk
    pub dirty: bool,
    /* metadata used by the VFS*/
    /// The current size of the file or directory
    pub size: usize,
    pub accessed_at: u64,
    pub created_at: u64,
    pub modified_at: u64,
    /* other */
    /// Container which may be used by the FS implementation to store additional
    /// data with this FsNode
    pub private_data: Option<Box<dyn Any + Send + Sync>>,
}

impl PartialEq for FsNode {
    fn eq(&self, other: &Self) -> bool {
        // the id and mount id is enough to uniquely identify this node and
        // compare against other nodes
        self.id == other.id && self.mount_id == other.mount_id
    }
}

impl FsNode {
    #[track_caller]
    pub fn data_as<T: 'static>(&self) -> &T {
        self.private_data
            .as_ref()
            .unwrap()
            .downcast_ref::<T>()
            .unwrap()
    }

    pub fn file_system(&self) -> Arc<dyn FileSystem> {
        vfs::get()
            .get_mount(self.mount_id)
            .expect("FsNodes which exist should have a valid mount in the mount table")
            .file_system
            .clone()
    }

    pub fn is_directory(&self) -> bool {
        self.kind == FsNodeKind::Directory
    }

    pub fn is_file(&self) -> bool {
        self.kind == FsNodeKind::File
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FsNodeId(u64);

impl FsNodeId {
    pub const ZERO: Self = Self(0);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FsNodeKind {
    Directory,
    File,
    CharDevice,
    BlockDevice,
}

impl Display for FsNodeKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                FsNodeKind::Directory => "d",
                FsNodeKind::File => "-",
                FsNodeKind::CharDevice => "c",
                FsNodeKind::BlockDevice => "d",
            }
        )
    }
}

/// Represents an opened file
pub struct File {
    /// The backing VFS node which this file is an opened instance of
    pub node: Arc<FsNode>,
    /// The mode which this file is opened with
    pub mode: FileMode,
    /// The current position into the file (cursor)
    pub position: Mutex<usize>,
    /// Container which may be used by the FS or device driver implementation to
    /// store additional data with this open file. Should be initialized by
    /// [`FileOperations::open`] and cleaned up by [`FileOperations::close`]
    pub private_data: Option<Box<dyn Any + Send + Sync>>,
}

/// Uniquely identifies an open file
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileDescriptor(u64);

impl FileDescriptor {
    pub const NULL: Self = Self(0);

    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    Read,
    Write,
    Append,
}

impl FileMode {
    pub fn is_mutating(self) -> bool {
        match self {
            FileMode::Read => false,
            FileMode::Write | FileMode::Append => true,
        }
    }
}

impl File {
    pub fn new(node: Arc<FsNode>, mode: FileMode) -> Self {
        Self {
            node,
            mode,
            position: Mutex::new(0),
            private_data: None,
        }
    }

    pub fn new_with_data(
        node: Arc<FsNode>,
        mode: FileMode,
        data: Box<dyn Any + Send + Sync>,
    ) -> Self {
        Self {
            node,
            mode,
            position: Mutex::new(0),
            private_data: Some(data),
        }
    }

    pub fn file_system(&self) -> Arc<dyn FileSystem> {
        vfs::get()
            .get_mount(self.node.mount_id)
            .expect("Files which exist should have a valid mount in the mount table")
            .file_system
            .clone()
    }
}

/// Initializes the file subsystem. Allocates the memory required for the
/// virtual file system and loads required fs drivers
pub fn init() {
    drivers::fs::init().expect("Failed to initialize file system drivers");
    vfs::init();
}
