use crate::{
    device::char::{CharDevice, CharacterDeviceMetadata},
    fs::{File, FileOperations, vfs::IoError},
};

pub struct ZeroDevice;

impl CharDevice for ZeroDevice {
    fn metadata(&self) -> &CharacterDeviceMetadata {
        &CharacterDeviceMetadata { name: "zero" }
    }

    fn file_operations(&self) -> &dyn FileOperations {
        self
    }
}

impl FileOperations for ZeroDevice {
    fn read(&self, _file: &File, _offset: usize, buffer: &mut [u8]) -> Result<usize, IoError> {
        buffer.fill(0);
        Ok(buffer.len())
    }
}
