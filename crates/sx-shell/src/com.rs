//! Per-thread COM initialisation guard (`.claude/rules/windows-ffi.md`).

use std::marker::PhantomData;

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};

use crate::error::ShellError;

/// Keeps the calling thread in a single-threaded apartment while alive. Not `Send`: COM must be
/// uninitialised on the thread that initialised it.
#[derive(Debug)]
pub struct StaGuard {
    _thread_bound: PhantomData<*const ()>,
}

impl StaGuard {
    /// Enters an STA. A thread that is already in an STA is fine (`S_FALSE`, balanced on drop);
    /// one that is already in the MTA is an error, because STA-only shell objects would fail.
    pub fn new() -> Result<Self, ShellError> {
        // SAFETY: no reserved pointer; balanced by CoUninitialize in Drop when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if hr == RPC_E_CHANGED_MODE {
            return Err(ShellError::Apartment);
        }
        hr.ok().map_err(ShellError::Com)?;
        Ok(Self {
            _thread_bound: PhantomData,
        })
    }
}

impl Drop for StaGuard {
    fn drop(&mut self) {
        // SAFETY: matches the successful CoInitializeEx (S_OK or S_FALSE) on this same thread;
        // the guard is not Send.
        unsafe { CoUninitialize() };
    }
}
