//! Floppy Disk Driver

use crate::device::block::{BlockDevice, BlockDeviceIoError, BlockDeviceMetadata};

pub struct FloppyDisk {
    drive_id: u8,
    direction: bool,
    step_index: u8,
}

impl FloppyDisk {
    /// Callers must ensure that only one instance of this driver exists for
    /// each drive ID
    pub unsafe fn new(drive_id: u8) -> Self {
        Self {
            drive_id,
            direction: false,
            step_index: 0,
        }
    }

    /// Resets the floppy disk to a known state. Should be called after
    /// instantiation.
    pub fn reset(&mut self) {}
}

impl BlockDevice for FloppyDisk {
    fn metadata(&self) -> BlockDeviceMetadata {
        BlockDeviceMetadata {
            block_size: 512,
            total_blocks: 2880,
        }
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, BlockDeviceIoError> {
        todo!()
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, BlockDeviceIoError> {
        todo!()
    }
}
