use alloc::sync::Arc;

use null::NullDevice;
use zero::ZeroDevice;

use crate::device::char::{CharDeviceRegistrationError, register_char_device};

mod null;
mod zero;

pub fn init() -> Result<(), CharDeviceRegistrationError> {
    register_char_device(Arc::new(NullDevice))?;
    register_char_device(Arc::new(ZeroDevice))?;

    Ok(())
}
