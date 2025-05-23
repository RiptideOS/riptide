use alloc::{collections::BTreeMap, sync::Arc};

use spin::Mutex;

use super::FileSystemType;

lazy_static::lazy_static! {
    // Maps file systems from names to implementations
    static ref FILE_SYSTEM_REGISTRY: Mutex<BTreeMap<&'static str, Arc<dyn FileSystemType>>>
        = Default::default();
}

#[derive(Debug)]
pub enum FileSystemRegistrationError {
    NameConflict,
    MagicConflict,
}

/// Registers a file system type to be used when mounting and detecting file
/// systems from devices
pub fn register_file_system(
    fs: Arc<dyn FileSystemType>,
) -> Result<(), FileSystemRegistrationError> {
    let mut registry = FILE_SYSTEM_REGISTRY.lock();

    let name = fs.metadata().name;

    // Make sure no other file systems are registered under this name
    if registry.contains_key(name) {
        return Err(FileSystemRegistrationError::NameConflict);
    }

    // FIXME: add this back
    
    // // Make sure no other file systems are registered with the same magic bytes
    // if registry
    //     .values()
    //     .any(|f| f.metadata().magic == fs.metadata().magic)
    // {
    //     return Err(FileSystemRegistrationError::MagicConflict);
    // }

    registry.insert(name, fs);

    Ok(())
}

/// Gets a file system by name for mounting purposes
pub fn find_file_system_type(name: &str) -> Option<Arc<dyn FileSystemType>> {
    let registry = FILE_SYSTEM_REGISTRY.lock();

    registry.get(name).cloned()
}
