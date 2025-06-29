/// Represents an abstract device which can read and write data to/from a store
/// in fixed size blocks
pub trait BlockDevice {
    fn metadata(&self) -> BlockDeviceMetadata;

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, BlockDeviceIoError> {
        Err(BlockDeviceIoError::OperationNotSupported)
    }

    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, BlockDeviceIoError> {
        Err(BlockDeviceIoError::OperationNotSupported)
    }
}

pub struct BlockDeviceMetadata {
    pub block_size: usize,
    pub total_blocks: usize,
}

pub enum BlockDeviceIoError {
    /// Returned if this operation is not supported on this device
    OperationNotSupported,
    /// The provided offset was not aligned to the block size
    UnalignedOffset,
    /// The provided offset was out of range for the device
    OffsetOutOfBounds,
    /// The provided buffer was not a multiple of the block size
    MismatchedBlockSize,
}
