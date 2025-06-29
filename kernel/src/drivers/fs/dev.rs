use alloc::{boxed::Box, sync::Arc};

use spin::Mutex;

use crate::{
    device::char::{CharDevice, get_char_device, list_char_devices},
    fs::{
        DirectoryOperations, File, FileOperations, FileSystem, FileSystemMetadata, FileSystemType,
        FileSystemTypeMetadata, FsNode, FsNodeId, FsNodeKind, FsNodeLock, FsNodeMetadata,
        FsNodeOperations, MountFlags, impl_fs_ops_for_self,
        vfs::{DirectoryEntry, DirectoryIterationContext, IoError, MountId},
    },
    util::sync_cell::SynCell,
};

pub struct DevFileSystemType;

impl FileSystemType for DevFileSystemType {
    fn metadata(&self) -> &FileSystemTypeMetadata {
        &FileSystemTypeMetadata {
            name: "devfs",
            magic: &[],
        }
    }

    fn mount(
        self: Arc<Self>,
        mount_id: MountId,
        source: &str,
        flags: MountFlags,
    ) -> Result<Arc<dyn FileSystem>, IoError> {
        assert_eq!(source, "", "dev does not take a source argument");

        Ok(Arc::new(DevFileSystem {
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
                private_data: None,
            }),
            next_node_id: SynCell::new(FsNodeId::new(1)),
        }))
    }

    fn unmount(self: Arc<Self>, _instance: Arc<dyn FileSystem>) {
        todo!("unmount dev file system")
    }
}

pub struct DevFileSystem {
    metadata: FileSystemMetadata,
    next_node_id: SynCell<FsNodeId>,
    root: Arc<FsNode>,
}

impl DevFileSystem {
    fn next_node_id(&self) -> FsNodeId {
        self.next_node_id
            .replace(|id| FsNodeId::new(id.as_u64() + 1))
    }
}

impl FileSystem for DevFileSystem {
    fn metadata(&self) -> &FileSystemMetadata {
        &self.metadata
    }

    fn root_directory(&self) -> Arc<FsNode> {
        self.root.clone()
    }

    impl_fs_ops_for_self!();
}

impl FsNodeOperations for DevFileSystem {
    fn write_node(&self, _node: &FsNode) -> Result<(), ()> {
        // no-op because we dont persist files
        Ok(())
    }

    fn evict_node(&self, _node: &FsNode) -> Result<(), ()> {
        // no-op because we dont persist files
        Ok(())
    }
}

impl FileOperations for DevFileSystem {
    fn read(&self, file: &File, offset: usize, buffer: &mut [u8]) -> Result<usize, IoError> {
        match file.node.kind {
            FsNodeKind::CharDevice => {
                let c_dev = file.node.data_as::<Arc<dyn CharDevice>>();

                c_dev.file_operations().read(file, offset, buffer)
            }
            FsNodeKind::BlockDevice => todo!(),
            _ => unreachable!(),
        }
    }

    fn write(&self, file: &File, offset: usize, buffer: &[u8]) -> Result<usize, IoError> {
        match file.node.kind {
            FsNodeKind::CharDevice => {
                let c_dev = file.node.data_as::<Arc<dyn CharDevice>>();

                c_dev.file_operations().write(file, offset, buffer)
            }
            FsNodeKind::BlockDevice => todo!(),
            _ => unreachable!(),
        }
    }
}

impl DirectoryOperations for DevFileSystem {
    fn lookup(
        &self,
        _parent: &Arc<DirectoryEntry>,
        name: &str,
    ) -> Result<Option<Arc<FsNode>>, IoError> {
        // We only support a single directory right now, so just lookup the name
        // in the device table

        Ok(get_char_device(name).map(|d| {
            Arc::new(FsNode {
                // FIXME: see below comment about consistent node ids
                id: self.next_node_id(),
                mount_id: self.root.mount_id,
                kind: FsNodeKind::CharDevice,
                metadata: Mutex::new(FsNodeMetadata {
                    dirty: false,
                    link_count: 1,
                    // FIXME: what should these be?
                    size: 0,
                    accessed_at: 0,
                    created_at: 0,
                    modified_at: 0,
                }),
                structure_lock: Mutex::new(FsNodeLock),
                private_data: Some(Box::new(d)),
            })
        }))
    }

    fn read_directory(
        &self,
        context: &mut DirectoryIterationContext,
        _entry: &Arc<DirectoryEntry>,
    ) -> Result<(), IoError> {
        // We only support a single directory right now, so just list all
        // devices currently registered in the device table

        for dev in list_char_devices() {
            context.insert(
                dev.metadata().name,
                // FIXME: we should always be returning the same fsnode ids for
                // any given device but for now this is ok. can we assign global
                // ids to each device and then store a mapping from device ids
                // to node ids? should the device ids just be the node ids?
                self.next_node_id(),
                FsNodeKind::CharDevice,
            );
        }
        Ok(())
    }
}
