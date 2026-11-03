//! The single-instance pipe (ARCHITECTURE.md §12, SYSTEM-INTEGRATION.md §4).
//!
//! `\\.\pipe\switch-explorer.<user SID>.<session>[.admin].v1`:
//! - The name is per user, logon session and elevation. Pipe names are machine-wide, so a second
//!   desktop session or an elevated window keeps its own instance.
//! - The DACL grants only the current user and SYSTEM.
//! - Remote clients are rejected.
//! - The first instance is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, which is the
//!   single-instance arbitration: a second process cannot create the pipe, so it connects to it
//!   and forwards its request.
//! - Anyone may create a pipe with this name first. A client therefore only talks to a server
//!   that runs as the same user, session and elevation from the same executable, and it lets the
//!   server identify it but never impersonate it (SECURITY.md §6).
//!
//! All I/O is overlapped with a deadline per message, so a client that connects and then stalls
//! cannot block the server, and a server that stops answering cannot block a client.

use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NO_DATA,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_ELEVATION,
    TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenSessionId, TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, ResetEvent, SetEvent,
    WaitForMultipleObjects, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
use windows::core::{PCWSTR, PWSTR};

use crate::error::IpcError;
use crate::protocol::{self, Message, VERSION};

/// How long a connected peer may take to send or answer one message.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const BUFFER: u32 = 64 * 1024;

/// A kernel handle closed on drop.
#[derive(Debug)]
struct Owned(HANDLE);

// SAFETY: kernel handles may be used and closed from any thread.
unsafe impl Send for Owned {}
// SAFETY: as above; the handles here are only signalled/waited on, which is thread-safe.
unsafe impl Sync for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the handle is owned by this guard and closed exactly once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

fn event() -> Result<Owned, IpcError> {
    // SAFETY: an unnamed manual-reset event, owned by the returned guard.
    let handle = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }?;
    Ok(Owned(handle))
}

/// One token information class, in u64 storage so the pointer-aligned structures are aligned.
/// `min` is the size of a fixed-size class, for which the size query may not report a length.
fn token_info(
    token: HANDLE,
    class: TOKEN_INFORMATION_CLASS,
    min: usize,
) -> Result<Vec<u64>, IpcError> {
    let mut len = 0u32;
    // SAFETY: a size query; it fails with ERROR_INSUFFICIENT_BUFFER and sets `len`.
    let _ = unsafe { GetTokenInformation(token, class, None, 0, &raw mut len) };
    let bytes = (len as usize).max(min);
    let mut buffer = vec![0u64; bytes.div_ceil(8).max(1)];
    // SAFETY: the buffer holds at least `bytes` bytes and outlives the call.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            bytes as u32,
            &raw mut len,
        )
    }?;
    Ok(buffer)
}

/// Who a process runs as: the account, the logon session and whether it is elevated.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    /// `S-1-5-21-…`.
    sid: String,
    session: u32,
    elevated: bool,
}

impl Identity {
    fn of_token(token: HANDLE) -> Result<Self, IpcError> {
        let user = token_info(token, TokenUser, size_of::<TOKEN_USER>())?;
        // SAFETY: GetTokenInformation(TokenUser) filled the buffer with a TOKEN_USER whose SID
        // pointer points into the same buffer, which is alive for the conversion below.
        let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut text = PWSTR::null();
        // SAFETY: `sid` is valid (see above); the string is freed with LocalFree after copying.
        let sid = unsafe {
            ConvertSidToStringSidW(sid, &raw mut text)?;
            let out = text.to_string().map_err(|_| IpcError::Closed);
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            out
        }?;
        let session = token_info(token, TokenSessionId, size_of::<u32>())?;
        let elevation = token_info(token, TokenElevation, size_of::<TOKEN_ELEVATION>())?;
        // SAFETY: both buffers were filled by GetTokenInformation with the fixed-size structure
        // of their class (a DWORD session id, a TOKEN_ELEVATION) and are at least that large.
        let (session, elevated) = unsafe {
            (
                *session.as_ptr().cast::<u32>(),
                (*elevation.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated != 0,
            )
        };
        Ok(Self {
            sid,
            session,
            elevated,
        })
    }

    fn of_process(process: HANDLE) -> Result<Self, IpcError> {
        let mut token = HANDLE::default();
        // SAFETY: query access to the token of a process handle we hold; closed by `Owned`.
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) }?;
        let token = Owned(token);
        Self::of_token(token.0)
    }

    fn current() -> Result<Self, IpcError> {
        // SAFETY: the pseudo handle of this process needs no closing.
        Self::of_process(unsafe { GetCurrentProcess() })
    }
}

/// The pipe name for this user, session, elevation and protocol version.
pub fn pipe_name() -> Result<String, IpcError> {
    let me = Identity::current()?;
    let admin = if me.elevated { ".admin" } else { "" };
    Ok(format!(
        r"\\.\pipe\switch-explorer.{}.{}{admin}.v{VERSION}",
        me.sid, me.session
    ))
}

/// The executable of a process, resolved so two spellings of one file compare equal.
fn image_path(process: HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0u16; 32 * 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: `buffer` holds `len` UTF-16 units; the call writes at most that many and stores
    // the length written in `len`.
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut len,
        )
    }
    .ok()?;
    buffer.truncate(len as usize);
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer));
    std::fs::canonicalize(path).ok()
}

/// The process id of the server at the other end of `pipe` when it is a Switch Explorer of this
/// user: same account, session and elevation, same executable as this process. A process that
/// merely created a pipe with our name first fails this, and gets neither the request nor the
/// right to take the foreground.
fn trusted_server(pipe: HANDLE) -> Result<u32, IpcError> {
    let mut pid = 0u32;
    // SAFETY: `pipe` is a connected client handle; the pid is written to a local.
    unsafe { GetNamedPipeServerProcessId(pipe, &raw mut pid) }?;
    // SAFETY: limited query access to the server process; the handle is closed by `Owned`.
    let process = Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }?);
    if Identity::of_process(process.0)? != Identity::current()? {
        return Err(IpcError::Untrusted);
    }
    let ours = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::canonicalize(exe).ok());
    match (image_path(process.0), ours) {
        (Some(theirs), Some(ours)) if theirs == ours => Ok(pid),
        _ => Err(IpcError::Untrusted),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A security descriptor that grants the pipe to the current user and SYSTEM only.
struct Dacl(PSECURITY_DESCRIPTOR);

impl Dacl {
    fn current_user() -> Result<Self, IpcError> {
        let sddl = wide(&format!(
            "D:P(A;;GA;;;{})(A;;GA;;;SY)",
            Identity::current()?.sid
        ));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is freed with LocalFree on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }?;
        Ok(Self(descriptor))
    }
}

impl Drop for Dacl {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by ConvertStringSecurityDescriptor… with
        // LocalAlloc and is freed once.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// Runs one overlapped operation and waits for it, cancelling it after `timeout` (`None` waits
/// forever). Returns the bytes transferred; `ERROR_PIPE_CONNECTED` counts as done.
fn overlapped(
    handle: HANDLE,
    event: &Owned,
    timeout: Option<Duration>,
    start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>,
) -> Result<u32, IpcError> {
    // The event is manual-reset and reused: clear it so a completion of an earlier operation
    // cannot be mistaken for this one.
    // SAFETY: resets our own event.
    unsafe { ResetEvent(event.0) }?;
    let mut ov = OVERLAPPED {
        hEvent: event.0,
        ..Default::default()
    };
    match start(&raw mut ov) {
        Ok(()) => {}
        Err(err) if err.code() == ERROR_IO_PENDING.to_hresult() => {}
        Err(err) if err.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(0),
        Err(err) => return Err(err.into()),
    }
    let ms = timeout.map_or(u32::MAX, |t| {
        t.as_millis().min(u128::from(u32::MAX - 1)) as u32
    });
    // SAFETY: waits on our own event, which the pending operation signals.
    let waited = unsafe { WaitForSingleObject(event.0, ms) };
    let mut done = 0u32;
    if waited != WAIT_OBJECT_0 {
        let err = if waited == WAIT_TIMEOUT {
            IpcError::Timeout
        } else {
            windows::core::Error::from_thread().into()
        };
        // SAFETY: cancels this operation, then waits for it to finish so `ov` is not used after
        // this function returns (also when the wait itself failed).
        unsafe {
            let _ = CancelIoEx(handle, Some(&raw const ov));
            let _ = GetOverlappedResult(handle, &raw const ov, &raw mut done, true);
        }
        return Err(err);
    }
    // SAFETY: the operation completed (the event is signalled); `ov` and `done` are locals.
    unsafe { GetOverlappedResult(handle, &raw const ov, &raw mut done, false) }?;
    Ok(done)
}

/// Time left until `deadline`; `Timeout` once it has passed.
fn left(deadline: Instant) -> Result<Duration, IpcError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(IpcError::Timeout)
    } else {
        Ok(left)
    }
}

fn read_exact(
    handle: HANDLE,
    event: &Owned,
    out: &mut [u8],
    deadline: Instant,
) -> Result<(), IpcError> {
    let mut filled = 0;
    while filled < out.len() {
        let rest = &mut out[filled..];
        let ptr = rest.as_mut_ptr();
        let len = rest.len();
        let n = overlapped(handle, event, Some(left(deadline)?), |ov| {
            // SAFETY: `ptr..ptr+len` is a live, exclusively borrowed buffer that outlives the
            // operation (overlapped() waits for completion or cancellation).
            unsafe {
                ReadFile(
                    handle,
                    Some(std::slice::from_raw_parts_mut(ptr, len)),
                    None,
                    Some(ov),
                )
            }
        })?;
        if n == 0 {
            return Err(IpcError::Closed);
        }
        filled += n as usize;
    }
    Ok(())
}

fn write_all(
    handle: HANDLE,
    event: &Owned,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), IpcError> {
    let mut sent = 0;
    while sent < bytes.len() {
        let rest = &bytes[sent..];
        let n = overlapped(handle, event, Some(left(deadline)?), |ov| {
            // SAFETY: `rest` is borrowed for the whole operation (see read_exact).
            unsafe { WriteFile(handle, Some(rest), None, Some(ov)) }
        })?;
        if n == 0 {
            return Err(IpcError::Closed);
        }
        sent += n as usize;
    }
    Ok(())
}

/// Reads one message; the whole message must arrive within [`IO_TIMEOUT`], so a peer that
/// trickles bytes cannot hold the other side.
fn read_message(handle: HANDLE, event: &Owned) -> Result<Message, IpcError> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut header = [0u8; 4];
    read_exact(handle, event, &mut header, deadline)?;
    let mut body = vec![0u8; protocol::body_len(header)?];
    read_exact(handle, event, &mut body, deadline)?;
    protocol::decode(&body)
}

fn write_message(handle: HANDLE, event: &Owned, message: &Message) -> Result<(), IpcError> {
    let deadline = Instant::now() + IO_TIMEOUT;
    write_all(handle, event, &protocol::encode(message)?, deadline)
}

/// The pipe of the primary instance, created but not yet serving.
#[derive(Debug)]
pub struct Server {
    pipe: Owned,
}

/// Stops the server thread when dropped.
#[derive(Debug)]
pub struct ServerHandle {
    stop: std::sync::Arc<Owned>,
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        // SAFETY: signals our own event; the server thread holds another reference to it.
        let _ = unsafe { SetEvent(self.stop.0) };
    }
}

impl Server {
    /// Claims the pipe for this process. `AlreadyRunning` means another instance owns it: send
    /// it the request with [`send`] instead.
    pub fn bind() -> Result<Self, IpcError> {
        Self::bind_named(&pipe_name()?)
    }

    /// [`Self::bind`] with an explicit pipe name (tests).
    pub fn bind_named(name: &str) -> Result<Self, IpcError> {
        let name = wide(name);
        let dacl = Dacl::current_user()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: dacl.0.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: `name` is NUL-terminated; `attributes` and the descriptor it points to are
        // alive for the call (the pipe copies the descriptor).
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                BUFFER,
                BUFFER,
                0,
                Some(&raw const attributes),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            let err = windows::core::Error::from_thread();
            // ACCESS_DENIED: FILE_FLAG_FIRST_PIPE_INSTANCE lost; PIPE_BUSY: the single allowed
            // instance exists. Either way another process owns the name.
            let taken = [ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY]
                .iter()
                .any(|code| err.code() == code.to_hresult());
            return Err(if taken {
                IpcError::AlreadyRunning
            } else {
                err.into()
            });
        }
        Ok(Self { pipe: Owned(pipe) })
    }

    /// Serves requests on a dedicated thread until the handle is dropped. `handler` runs on that
    /// thread for every message and returns the reply; panics in it are caught.
    pub fn serve(
        self,
        mut handler: impl FnMut(Message) -> Message + Send + 'static,
    ) -> Result<ServerHandle, IpcError> {
        let stop = std::sync::Arc::new(event()?);
        let thread_stop = stop.clone();
        std::thread::Builder::new()
            .name("sx-ipc".into())
            .spawn(move || {
                if let Err(err) = self.run(&thread_stop, &mut handler) {
                    tracing::warn!(error = %err, "ipc server stopped");
                }
            })
            .map_err(IpcError::Thread)?;
        Ok(ServerHandle { stop })
    }

    fn run(
        &self,
        stop: &Owned,
        handler: &mut impl FnMut(Message) -> Message,
    ) -> Result<(), IpcError> {
        let pipe = self.pipe.0;
        let io = event()?;
        loop {
            // Wait for a client or for the stop signal.
            // SAFETY: resets our own event before reusing it (see `overlapped`).
            unsafe { ResetEvent(io.0) }?;
            let mut ov = OVERLAPPED {
                hEvent: io.0,
                ..Default::default()
            };
            // SAFETY: overlapped connect on our pipe; `ov` lives until the wait below ends or the
            // operation is cancelled.
            let started = unsafe { ConnectNamedPipe(pipe, Some(&raw mut ov)) };
            let connected = match started {
                Ok(()) => true,
                Err(err) if err.code() == ERROR_PIPE_CONNECTED.to_hresult() => true,
                Err(err) if err.code() == ERROR_IO_PENDING.to_hresult() => false,
                Err(err) if err.code() == ERROR_NO_DATA.to_hresult() => {
                    // A client connected and left before we got to it: reset and wait again.
                    // SAFETY: resets the instance so the next client can connect.
                    let _ = unsafe { DisconnectNamedPipe(pipe) };
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            if !connected {
                // SAFETY: both handles are live events owned by this thread's callers.
                let waited = unsafe { WaitForMultipleObjects(&[io.0, stop.0], false, u32::MAX) };
                if waited != WAIT_OBJECT_0 {
                    // Stop requested (or the wait failed): cancel the pending connect.
                    // SAFETY: cancels our own operation and waits for it so `ov` stays valid.
                    unsafe {
                        let _ = CancelIoEx(pipe, Some(&raw const ov));
                        let mut done = 0;
                        let _ = GetOverlappedResult(pipe, &raw const ov, &raw mut done, true);
                    }
                    return Ok(());
                }
                let mut done = 0;
                // SAFETY: the connect completed (its event is signalled); `ov` is a local.
                if unsafe { GetOverlappedResult(pipe, &raw const ov, &raw mut done, false) }
                    .is_err()
                {
                    // SAFETY: resets the instance so the next client can connect.
                    let _ = unsafe { DisconnectNamedPipe(pipe) };
                    continue;
                }
            }
            let reply = match read_message(pipe, &io) {
                Ok(message) => {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(message)))
                        .unwrap_or(Message::Unknown)
                }
                Err(err) => {
                    tracing::debug!(error = %err, "bad ipc request");
                    Message::Unknown
                }
            };
            if write_message(pipe, &io, &reply).is_ok() {
                // DisconnectNamedPipe discards unread data, so wait until the client has read the
                // reply and closed its end (or the timeout passes for a client that never does).
                let mut probe = [0u8; 1];
                let _ = read_exact(pipe, &io, &mut probe, Instant::now() + IO_TIMEOUT);
            }
            // SAFETY: drops the client so the next one can connect to this instance.
            let _ = unsafe { DisconnectNamedPipe(pipe) };
        }
    }
}

/// Sends `message` to the running instance and returns its reply. `NotRunning` when there is
/// none; `Untrusted` when the pipe belongs to a process that is not our running instance (see
/// the module docs). Lets the server take the foreground (it is about to show the window the
/// user asked for).
pub fn send(message: &Message, timeout: Duration) -> Result<Message, IpcError> {
    send_named(&pipe_name()?, message, timeout)
}

/// [`send`] to an explicit pipe name (tests).
pub fn send_named(name: &str, message: &Message, timeout: Duration) -> Result<Message, IpcError> {
    let name = wide(name);
    let deadline = Instant::now() + timeout;
    let pipe = loop {
        // SAFETY: `name` is NUL-terminated; the handle is owned by `Owned` below. The quality
        // of service limits the server to identifying us: it cannot impersonate this process.
        let opened = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        };
        match opened {
            Ok(handle) => break Owned(handle),
            Err(err) if err.code() == ERROR_FILE_NOT_FOUND.to_hresult() => {
                return Err(IpcError::NotRunning);
            }
            Err(err) if err.code() == ERROR_PIPE_BUSY.to_hresult() => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(IpcError::Timeout);
                }
                // SAFETY: `name` is NUL-terminated.
                let _ = unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), left.as_millis() as u32) };
            }
            Err(err) => return Err(err.into()),
        }
    };
    let server_pid = trusted_server(pipe.0).map_err(|err| {
        tracing::warn!(error = %err, "pipe server is not our running instance");
        IpcError::Untrusted
    })?;
    // SAFETY: plain call; failing only means the server may not take the foreground.
    let _ = unsafe { AllowSetForegroundWindow(server_pid) };
    let io = event()?;
    write_message(pipe.0, &io, message)?;
    read_message(pipe.0, &io)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_name(tag: &str) -> String {
        format!(r"\\.\pipe\sx-ipc-test-{}-{tag}", std::process::id())
    }

    #[test]
    fn round_trip_and_single_instance() {
        let name = test_name("rt");
        let server = Server::bind_named(&name).unwrap();
        assert!(matches!(
            Server::bind_named(&name),
            Err(IpcError::AlreadyRunning)
        ));
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = server
            .serve(move |message| {
                let _ = tx.send(message);
                Message::Ack
            })
            .unwrap();
        let open = Message::OpenPaths {
            paths: vec![std::path::PathBuf::from(r"C:\Users")],
            select: None,
            new_window: false,
            home: false,
        };
        for _ in 0..3 {
            assert_eq!(
                send_named(&name, &open, Duration::from_secs(2)).unwrap(),
                Message::Ack
            );
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), open);
        }
        drop(handle);
    }

    #[test]
    fn no_server_is_not_running() {
        let err = send_named(
            &test_name("none"),
            &Message::Ping,
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(matches!(err, IpcError::NotRunning), "{err:?}");
    }

    #[test]
    fn a_stalled_client_does_not_block_the_server() {
        let name = test_name("stall");
        let handle = Server::bind_named(&name)
            .unwrap()
            .serve(|_| Message::Pong)
            .unwrap();
        // Connect and send nothing; the server times this client out and serves the next one.
        let wide_name = wide(&name);
        // SAFETY: opens a client handle that is closed by `Owned`.
        let stalled = unsafe {
            CreateFileW(
                PCWSTR(wide_name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map(Owned)
        .unwrap();
        let reply = send_named(&name, &Message::Ping, Duration::from_secs(5)).unwrap();
        assert_eq!(reply, Message::Pong);
        drop(stalled);
        drop(handle);
    }

    #[test]
    fn pipe_name_is_per_user_and_session() {
        let me = Identity::current().unwrap();
        let name = pipe_name().unwrap();
        let prefix = format!(r"\\.\pipe\switch-explorer.{}.{}.", me.sid, me.session);
        assert!(me.sid.starts_with("S-1-"), "{name}");
        assert!(name.starts_with(&prefix), "{name}");
        assert_eq!(name.ends_with(".admin.v1"), me.elevated, "{name}");
        assert_eq!(name.rsplit('.').next(), Some("v1"));
    }

    #[test]
    fn a_squatting_server_is_not_trusted() {
        // Another program of the same user creates our pipe name first: the client refuses to
        // send it anything.
        let name = test_name("squat");
        let short = name.trim_start_matches(r"\\.\pipe\");
        let script = format!(
            "$s = New-Object System.IO.Pipes.NamedPipeServerStream('{short}'); \
             $s.WaitForConnection(); Start-Sleep -Seconds 5"
        );
        let mut squatter = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let result = loop {
            match send_named(&name, &Message::Ping, Duration::from_millis(200)) {
                Err(IpcError::NotRunning) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                other => break other,
            }
        };
        let _ = squatter.kill();
        let _ = squatter.wait();
        assert!(matches!(result, Err(IpcError::Untrusted)), "{result:?}");
    }

    #[test]
    fn this_process_is_a_trusted_server_of_itself() {
        // The round-trip tests talk to a server in this same process, so they pass the server
        // check; this pins the identity and image parts of it.
        // SAFETY: the pseudo handle of this process needs no closing.
        let process = unsafe { GetCurrentProcess() };
        assert_eq!(
            Identity::of_process(process).unwrap(),
            Identity::current().unwrap()
        );
        let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(image_path(process), Some(exe));
    }
}
