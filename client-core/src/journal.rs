use crate::error::{Error, Result};
pub fn require_supported_platform() -> Result<()> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err(Error::config(
            "Native file journals require a local POSIX filesystem. On Windows, use Linux/WSL with state in its Linux filesystem, not /mnt/c or /mnt/d. Keep existing journals for recovery; pending operations remain uncertain.",
        ))
    }
}
