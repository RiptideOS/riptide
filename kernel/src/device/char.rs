use alloc::{collections::btree_map::BTreeMap, sync::Arc, vec::Vec};

use spin::Mutex;

use crate::fs::FileOperations;

pub trait CharDevice: Send + Sync {
    fn metadata(&self) -> &CharacterDeviceMetadata;
    fn file_operations(&self) -> &dyn FileOperations;
}

pub struct CharacterDeviceMetadata {
    pub name: &'static str,
}

lazy_static::lazy_static! {
    // Maps file systems from names to implementations
    static ref CHAR_DEVICE_REGISTRY: Mutex<BTreeMap<&'static str, Arc<dyn CharDevice>>>
        = Default::default();
}

#[derive(Debug)]
pub enum CharDeviceRegistrationError {
    NameConflict,
}

pub fn register_char_device(c_dev: Arc<dyn CharDevice>) -> Result<(), CharDeviceRegistrationError> {
    let mut registry = CHAR_DEVICE_REGISTRY.lock();

    let name = c_dev.metadata().name;

    // Make sure no other devices are registered under this name
    if registry.contains_key(name) {
        return Err(CharDeviceRegistrationError::NameConflict);
    }

    registry.insert(name, c_dev);

    Ok(())
}

pub fn list_char_devices() -> Vec<Arc<dyn CharDevice>> {
    CHAR_DEVICE_REGISTRY.lock().values().cloned().collect()
}



pub fn get_char_device(name: &str) -> Option<Arc<dyn CharDevice>> {
    CHAR_DEVICE_REGISTRY.lock().get(name).cloned()
}
