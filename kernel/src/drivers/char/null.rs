use crate::{
    device::char::{CharDevice, CharacterDeviceMetadata},
    fs::{File, FileOperations, vfs::IoError},
};

pub struct NullDevice;

impl CharDevice for NullDevice {
    fn metadata(&self) -> &CharacterDeviceMetadata {
        &CharacterDeviceMetadata { name: "null" }
    }

    fn file_operations(&self) -> &dyn FileOperations {
        self
    }
}

impl FileOperations for NullDevice {
    fn write(&self, _file: &File, _offset: usize, buffer: &[u8]) -> Result<usize, IoError> {
        Ok(buffer.len())
    }
}
