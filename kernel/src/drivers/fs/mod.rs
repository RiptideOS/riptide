use alloc::sync::Arc;

use dev::DevFileSystemType;
use ram::RamFileSystemType;

use crate::fs::registry::{FileSystemRegistrationError, register_file_system};

mod dev;
mod ram;

pub fn init() -> Result<(), FileSystemRegistrationError> {
    register_file_system(Arc::new(RamFileSystemType))?;
    register_file_system(Arc::new(DevFileSystemType))?;

    Ok(())
}
