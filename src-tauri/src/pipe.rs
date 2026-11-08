//! The local named pipe between the app core and the elevated helper.
//!
//! * The core creates the pipe under a random name, first instance only, local clients only, with
//!   an access list that allows only the current Windows user.
//! * It only accepts the helper process it started (checked by process ID).
//! * The helper opens the pipe at identification level, so the core can never act with the
//!   helper's admin rights, and checks the server is the FrameWise process that started it.
//!
//! Messages are one JSON object per line.

use std::time::Duration;

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, OpenProcessToken, WaitForMultipleObjects};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const MAX_LINE: usize = 4 * 1024 * 1024;

pub struct Owned(pub HANDLE);
// SAFETY: kernel handles may be used from any thread.
unsafe impl Send for Owned {}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_invalid() && !self.0 .0.is_null() {
            // SAFETY: we own this handle.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

/// A 128-bit random pipe name, so another process can't guess it in advance.
pub fn random_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut parts = [0u64; 2];
    for (i, p) in parts.iter_mut().enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i as u64);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        *p = h.finish();
    }
    format!(r"\\.\pipe\framewise-{}-{:016x}{:016x}", std::process::id(), parts[0], parts[1])
}

/// The current user's SID as a string (`S-1-5-21-...`).
pub fn current_user_sid() -> Result<String, String> {
    // SAFETY: standard token query; buffers are sized by the first call and freed below.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(|e| e.to_string())?;
        let token = Owned(token);
        let mut len = 0u32;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr() as *mut _), len, &mut len).map_err(|e| e.to_string())?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).map_err(|e| e.to_string())?;
        let out = s.to_string().map_err(|e| e.to_string());
        let _ = LocalFree(HLOCAL(s.0 as *mut _));
        out
    }
}

pub struct Server {
    pipe: Owned,
    event: Owned,
    buf: Vec<u8>,
}

impl Server {
    pub fn create(name: &str) -> Result<Server, String> {
        let sid = current_user_sid()?;
        // Protected DACL: generic all for this user only.
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: sd is freed with LocalFree after the pipe is created.
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None) }
            .map_err(|e| format!("Couldn't secure the helper channel: {e}"))?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let wname = HSTRING::from(name);
        // SAFETY: wname and sa outlive the call.
        let pipe = unsafe {
            CreateNamedPipeW(
                &wname,
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64 * 1024,
                64 * 1024,
                0,
                Some(&sa),
            )
        };
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        let _ = unsafe { LocalFree(HLOCAL(sd.0)) };
        if pipe.is_invalid() {
            return Err(format!("Couldn't open the helper channel: {}", std::io::Error::last_os_error()));
        }
        let pipe = Owned(pipe);
        // SAFETY: manual-reset event for overlapped I/O.
        let event = Owned(unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(|e| e.to_string())?);
        Ok(Server { pipe, event, buf: Vec::new() })
    }

    /// Waits for one overlapped operation, or until `process` exits or the timeout passes.
    fn wait(&self, ov: &OVERLAPPED, process: HANDLE, timeout: Duration, what: &str) -> Result<u32, String> {
        let handles = [self.event.0, process];
        // SAFETY: both handles are valid for the duration of the wait.
        let r = unsafe { WaitForMultipleObjects(&handles, false, timeout.as_millis().min(u32::MAX as u128) as u32) };
        let mut n = 0u32;
        if r == WAIT_OBJECT_0 {
            // SAFETY: ov is the overlapped struct of the completed operation.
            return match unsafe { GetOverlappedResult(self.pipe.0, ov, &mut n, false) } {
                Ok(()) => Ok(n),
                Err(e) if e.code() == ERROR_BROKEN_PIPE.to_hresult() => Err("The helper closed the connection.".into()),
                Err(e) => Err(format!("Helper channel error: {e}")),
            };
        }
        // Cancel and wait for the cancellation so `ov` isn't used after this returns.
        // SAFETY: as above.
        unsafe {
            let _ = CancelIoEx(self.pipe.0, Some(ov));
            let _ = GetOverlappedResult(self.pipe.0, ov, &mut n, true);
        }
        if r == WAIT_TIMEOUT {
            Err(format!("The helper didn't respond in time ({what})."))
        } else {
            Err("The helper stopped unexpectedly.".into())
        }
    }

    /// Waits for the helper process `pid` to connect.
    pub fn accept(&self, process: HANDLE, pid: u32, timeout: Duration) -> Result<(), String> {
        let mut ov = OVERLAPPED { hEvent: self.event.0, ..Default::default() };
        // SAFETY: ov lives until the operation completes or is cancelled in `wait`.
        match unsafe { ConnectNamedPipe(self.pipe.0, Some(&mut ov)) } {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                self.wait(&ov, process, timeout, "connecting")?;
            }
            Err(e) => return Err(format!("Helper channel error: {e}")),
        }
        let mut client = 0u32;
        // SAFETY: connected pipe handle.
        unsafe { GetNamedPipeClientProcessId(self.pipe.0, &mut client) }.map_err(|e| e.to_string())?;
        if client != pid {
            return Err("An unexpected process connected to the helper channel, so FrameWise stopped.".into());
        }
        Ok(())
    }

    pub fn write_line(&self, line: &str, process: HANDLE) -> Result<(), String> {
        let mut data = line.as_bytes().to_vec();
        data.push(b'\n');
        let mut off = 0;
        while off < data.len() {
            let mut ov = OVERLAPPED { hEvent: self.event.0, ..Default::default() };
            // SAFETY: data and ov outlive the operation (waited on or cancelled below).
            let n = match unsafe { windows::Win32::Storage::FileSystem::WriteFile(self.pipe.0, Some(&data[off..]), None, Some(&mut ov)) } {
                Ok(()) => {
                    let mut n = 0;
                    unsafe { GetOverlappedResult(self.pipe.0, &ov, &mut n, true) }.map_err(|e| e.to_string())?;
                    n
                }
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => self.wait(&ov, process, Duration::from_secs(30), "sending")?,
                Err(e) => return Err(format!("Helper channel error: {e}")),
            };
            off += n as usize;
        }
        Ok(())
    }

    pub fn read_line(&mut self, process: HANDLE, timeout: Duration) -> Result<String, String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=pos).collect();
                return String::from_utf8(line[..line.len() - 1].to_vec()).map_err(|_| "The helper sent invalid text.".into());
            }
            if self.buf.len() > MAX_LINE {
                return Err("The helper sent too much data.".into());
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err("The helper didn't respond in time.".into());
            }
            let mut chunk = vec![0u8; 64 * 1024];
            let mut ov = OVERLAPPED { hEvent: self.event.0, ..Default::default() };
            // SAFETY: chunk and ov outlive the operation (waited on or cancelled below).
            let n = match unsafe { windows::Win32::Storage::FileSystem::ReadFile(self.pipe.0, Some(&mut chunk), None, Some(&mut ov)) } {
                Ok(()) => {
                    let mut n = 0;
                    unsafe { GetOverlappedResult(self.pipe.0, &ov, &mut n, true) }.map_err(|e| e.to_string())?;
                    n
                }
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => self.wait(&ov, process, left, "working")?,
                Err(e) if e.code() == ERROR_BROKEN_PIPE.to_hresult() => return Err("The helper closed the connection.".into()),
                Err(e) => return Err(format!("Helper channel error: {e}")),
            };
            if n == 0 {
                return Err("The helper closed the connection.".into());
            }
            self.buf.extend_from_slice(&chunk[..n as usize]);
        }
    }
}

/// Helper side: opens the pipe and checks that the server is `parent_pid` running this same exe.
pub fn connect_client(name: &str, parent_pid: u32) -> Result<std::fs::File, String> {
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    };
    use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
    if !name.starts_with(r"\\.\pipe\framewise-") || name.len() > 120 {
        return Err("bad pipe name".into());
    }
    let wname = HSTRING::from(name);
    let mut last = String::new();
    for _ in 0..50 {
        // Identification level: the server may learn who we are but can't impersonate us.
        // SAFETY: wname outlives the call.
        match unsafe {
            CreateFileW(
                &wname,
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                HANDLE::default(),
            )
        } {
            Ok(h) => {
                let h = Owned(h);
                let mut server = 0u32;
                // SAFETY: connected pipe handle.
                unsafe { GetNamedPipeServerProcessId(h.0, &mut server) }.map_err(|e| e.to_string())?;
                if server != parent_pid || !same_exe(server) {
                    return Err("The helper channel isn't owned by FrameWise.".into());
                }
                let raw = h.0 .0;
                std::mem::forget(h);
                // SAFETY: ownership of the handle moves into the File.
                return Ok(unsafe { std::fs::File::from_raw_handle(raw) });
            }
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(format!("Couldn't reach FrameWise: {last}"))
}

/// Whether process `pid` runs the same executable as this process.
fn same_exe(pid: u32) -> bool {
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    let Ok(me) = std::env::current_exe() else { return false };
    // SAFETY: handle closed by Owned; buffer sized.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let h = Owned(h);
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        if QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_err() {
            return false;
        }
        let theirs = String::from_utf16_lossy(&buf[..len as usize]);
        theirs.eq_ignore_ascii_case(&me.to_string_lossy())
    }
}
