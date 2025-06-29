use alloc::{boxed::Box, collections::btree_map::BTreeMap, sync::Arc, vec::Vec};

use spin::{Mutex, RwLock};

use crate::{
    fs::{
        DirectoryOperations, File, FileOperations, FileSystem, FileSystemMetadata, FileSystemType,
        FileSystemTypeMetadata, FsNode, FsNodeId, FsNodeKind, FsNodeLock, FsNodeMetadata,
        FsNodeOperations, MountFlags, impl_fs_ops_for_self,
        vfs::{DirectoryEntry, DirectoryIterationContext, IoError, MountId},
    },
    util::sync_cell::SynCell,
};

pub struct RamFileSystemType;

impl FileSystemType for RamFileSystemType {
    fn metadata(&self) -> &FileSystemTypeMetadata {
        &FileSystemTypeMetadata {
            name: "ramfs",
            magic: &[],
        }
    }

    fn mount(
        self: Arc<Self>,
        mount_id: MountId,
        source: &str,
        flags: MountFlags,
    ) -> Result<Arc<dyn FileSystem>, IoError> {
        assert_eq!(source, "", "ramfs does not take a source argument");

        Ok(Arc::new(RamFileSystem {
            metadata: FileSystemMetadata {
                device: None,
                mount_flags: flags,
                block_size: 512,
                max_file_size: usize::MAX,
                file_system_type: self.clone(),
            },
            root: Arc::new(FsNode {
                mount_id,
                id: FsNodeId::ZERO,
                kind: FsNodeKind::Directory,
                metadata: Mutex::new(FsNodeMetadata {
                    dirty: false,
                    link_count: 1,
                    size: 0,
                    accessed_at: 0,
                    created_at: 0,
                    modified_at: 0,
                }),
                structure_lock: Mutex::new(FsNodeLock),
                private_data: Some(Box::new(RamDirectoryNode::default())),
            }),
            next_node_id: SynCell::new(FsNodeId::new(1)),
        }))
    }

    fn unmount(self: Arc<Self>, _instance: Arc<dyn FileSystem>) {
        todo!("unmount ram file system")
    }
}

pub struct RamFileSystem {
    metadata: FileSystemMetadata,
    next_node_id: SynCell<FsNodeId>,
    root: Arc<FsNode>,
}

impl RamFileSystem {
    fn next_node_id(&self) -> FsNodeId {
        self.next_node_id
            .replace(|id| FsNodeId::new(id.as_u64() + 1))
    }
}

impl FileSystem for RamFileSystem {
    fn metadata(&self) -> &FileSystemMetadata {
        &self.metadata
    }

    fn root_directory(&self) -> Arc<FsNode> {
        self.root.clone()
    }

    impl_fs_ops_for_self!();
}

impl FsNodeOperations for RamFileSystem {
    fn write_node(&self, _node: &FsNode) -> Result<(), ()> {
        // no-op because we dont persist files
        Ok(())
    }

    fn evict_node(&self, _node: &FsNode) -> Result<(), ()> {
        // no-op because we dont persist files
        Ok(())
    }
}

#[derive(Default)]
pub struct RamFileNode {
    data: RwLock<Vec<u8>>,
}

impl FileOperations for RamFileSystem {
    fn read(&self, file: &File, offset: usize, buffer: &mut [u8]) -> Result<usize, IoError> {
        let f_node = file.node.data_as::<RamFileNode>();
        let data = f_node.data.read();

        // If the offset is past the end of the file, there is nothing to read
        if offset > data.len() {
            return Ok(0);
        }

        // The number of bytes we can read is determined by the number of bytes
        // left past the offset and the length of the buffer
        let bytes_remaining = data.len() - offset;
        let read_size = buffer.len().min(bytes_remaining);

        buffer[..read_size].copy_from_slice(&data[offset..offset + read_size]);

        Ok(read_size)
    }

    fn write(&self, file: &File, offset: usize, buffer: &[u8]) -> Result<usize, IoError> {
        let node = file.node.data_as::<RamFileNode>();
        let mut data = node.data.write();

        // If the length of the file would be increased by this operation, we
        // need to first resize the backing buffer up to the new length which
        // fills the new space (and any created holes) with 0s.
        let min_new_len = offset + buffer.len();
        if min_new_len > data.len() {
            data.resize(min_new_len, 0);
        }

        data[offset..offset + buffer.len()].copy_from_slice(buffer);

        Ok(buffer.len())
    }
}
#[derive(Default)]
pub struct RamDirectoryNode {
    // NOTE: we use a map from names to FsNodes instead of directory entries
    // because other file systems can be mounted over the children of this
    // directory. keeping this separate allows the cache to manage the
    // resolution more easily.
    children: RwLock<BTreeMap<Arc<str>, Arc<FsNode>>>,
}

impl DirectoryOperations for RamFileSystem {
    fn create_file(
        &self,
        parent: &Arc<DirectoryEntry>,
        name: &str,
    ) -> Result<Arc<FsNode>, IoError> {
        let node = Arc::new(FsNode {
            id: self.next_node_id(),
            mount_id: self.root.mount_id,
            kind: FsNodeKind::File,
            metadata: Mutex::new(FsNodeMetadata {
                dirty: false,
                link_count: 1,
                size: 0,
                accessed_at: 0,
                created_at: 0,
                modified_at: 0,
            }),
            structure_lock: Mutex::new(FsNodeLock),
            private_data: Some(Box::new(RamFileNode::default())),
        });

        let parent = parent.node.data_as::<RamDirectoryNode>();
        parent.children.write().insert(name.into(), node.clone());

        Ok(node)
    }

    fn create_directory(
        &self,
        parent: &Arc<DirectoryEntry>,
        name: &str,
    ) -> Result<Arc<FsNode>, IoError> {
        let node = Arc::new(FsNode {
            id: self.next_node_id(),
            mount_id: self.root.mount_id,
            kind: FsNodeKind::Directory,
            metadata: Mutex::new(FsNodeMetadata {
                dirty: false,
                link_count: 1,
                size: 0,
                accessed_at: 0,
                created_at: 0,
                modified_at: 0,
            }),
            structure_lock: Mutex::new(FsNodeLock),
            private_data: Some(Box::new(RamDirectoryNode::default())),
        });

        // FIXME: check if already exists

        let parent = parent.node.data_as::<RamDirectoryNode>();
        parent.children.write().insert(name.into(), node.clone());

        Ok(node)
    }

    fn remove_file(&self, parent: &Arc<DirectoryEntry>, name: &str) -> Result<(), IoError> {
        let parent = parent.node.data_as::<RamDirectoryNode>();

        if parent.children.write().remove(name).is_none() {
            return Err(IoError::EntryNotFound);
        }

        Ok(())
    }

    fn remove_directory(&self, parent: &Arc<DirectoryEntry>, name: &str) -> Result<(), IoError> {
        // NOTE: this function is called with the locks for both the parent and
        // the child held. This prevents them from being modified while we are
        // trying to remove them
        let parent = parent.node.data_as::<RamDirectoryNode>();

        let children = parent.children.write();

        let dir = children.get(name).ok_or(IoError::EntryNotFound)?;

        assert!(dir.is_directory());
        let dir_node = dir.data_as::<RamDirectoryNode>();
        assert!(dir_node.children.read().is_empty());

        // fixme: check child is empty
        todo!()
    }

    fn lookup(
        &self,
        parent: &Arc<DirectoryEntry>,
        name: &str,
    ) -> Result<Option<Arc<FsNode>>, IoError> {
        let d_node = parent.node.data_as::<RamDirectoryNode>();

        Ok(d_node.children.read().get(name).cloned())
    }

    fn read_directory(
        &self,
        context: &mut DirectoryIterationContext,
        directory: &Arc<DirectoryEntry>,
    ) -> Result<(), IoError> {
        let d_node = directory.node.data_as::<RamDirectoryNode>();

        for (name, node) in d_node.children.read().iter() {
            context.insert(name, node.id, node.kind);
        }

        Ok(())
    }
}
