//! The handles of this process, kept out of the server it starts.
//!
//! On Windows a new process inherits every handle of its parent that is marked
//! inheritable, whatever it is told to use as its own standard streams. The
//! shell that started this command passed it its pipes or files as such
//! handles, under more than the three standard values. A server that
//! inherited them would hold open the pipe or the file the shell reads this
//! command's output from, and the shell would wait for it for as long as the
//! server runs. So before the server is started every handle of this process
//! is marked not inheritable; this process can still use them. The handles the
//! server is given on purpose (its log and its closed input) are duplicated as
//! inheritable by `Command` at the moment it starts the server, after this.

/// The largest handle value looked at. A value is four times the index in the
/// process's handle table, and a command that starts one server holds far
/// fewer than the 4096 entries this reaches.
#[cfg(windows)]
const LAST_HANDLE_VALUE: usize = 0x4000;

/// Marks every handle of this process as not inheritable by the processes it
/// starts.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn keep_inherited_handles_from_children() {
    use std::ptr::without_provenance_mut;
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };
    for value in (4..=LAST_HANDLE_VALUE).step_by(4) {
        let handle = without_provenance_mut(value);
        let mut flags = 0_u32;
        // SAFETY: `flags` is a live, writable `u32` for the call; a value that is not an open handle of this process makes the call fail and changes nothing.
        let known = unsafe { GetHandleInformation(handle, &mut flags) } != 0;
        if known && flags & HANDLE_FLAG_INHERIT != 0 {
            // SAFETY: the call above succeeded, so `handle` is open in this process; clearing one flag of it touches no memory.
            // The result is not needed: a handle that cannot be flagged is no more inherited than before.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

/// Other systems replace the three standard descriptors of the child with the
/// ones `Stdio` names, and every descriptor this process opens is
/// close-on-exec, so nothing is left to do.
#[cfg(not(windows))]
pub fn keep_inherited_handles_from_children() {}
