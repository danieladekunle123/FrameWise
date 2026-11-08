//! Process helpers for game profiles. Everything here runs as the signed-in user, unelevated, and
//! only works on that user's own processes in their own session: Windows refuses the rest.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, FILETIME, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SUSPEND_RESUME, PROCESS_SYNCHRONIZE,
};

/// A process handle closed on drop.
pub struct Handle(pub HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: our own handle, closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn open(pid: u32, access: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS) -> Option<Handle> {
    // SAFETY: plain open; the handle is owned by Handle.
    unsafe { OpenProcess(access, false, pid) }.ok().map(Handle)
}

/// Every process as (pid, exe file name), from a Toolhelp snapshot (cheap, no handles opened).
pub fn list() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    // SAFETY: snapshot handle closed below; entry sized correctly.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

pub fn image_path(pid: u32) -> Option<String> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: buffer and length match.
    unsafe { QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// Process start time (FILETIME as u64): with the PID it identifies one process for good.
pub fn creation_time(pid: u32) -> Option<u64> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: out-pointers to locals.
    unsafe { GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u) }.ok()?;
    Some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
}

pub fn is_running(pid: u32) -> bool {
    const STILL_ACTIVE: u32 = 259;
    let Some(h) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else { return false };
    let mut code = 0u32;
    // SAFETY: out-pointer to a local.
    unsafe { GetExitCodeProcess(h.0, &mut code) }.is_ok() && code == STILL_ACTIVE
}

/// The Terminal Services session a process runs in (profiles only touch the user's own session).
pub fn session_of(pid: u32) -> Option<u32> {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    let mut s = 0u32;
    // SAFETY: out-pointer to a local.
    unsafe { ProcessIdToSessionId(pid, &mut s) }.ok().map(|_| s)
}

/// A handle that becomes signalled when the process exits, if Windows lets us open one.
pub fn wait_handle(pid: u32) -> Option<Handle> {
    open(pid, PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION)
}

fn ntdll(name: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
    // SAFETY: ntdll is always loaded; names are NUL-terminated.
    unsafe {
        let m = GetModuleHandleW(windows::core::w!("ntdll.dll")).ok()?;
        GetProcAddress(m, windows::core::PCSTR(name.as_ptr()))
    }
}

fn suspend_resume(pid: u32, resume: bool) -> bool {
    type F = unsafe extern "system" fn(HANDLE) -> i32;
    let Some(f) = ntdll(if resume { b"NtResumeProcess\0" } else { b"NtSuspendProcess\0" }) else { return false };
    // SAFETY: NtSuspendProcess/NtResumeProcess take one process handle and return an NTSTATUS.
    let f: F = unsafe { std::mem::transmute(f) };
    let Some(h) = open(pid, PROCESS_SUSPEND_RESUME) else { return false };
    unsafe { f(h.0) >= 0 }
}

/// Pauses every thread of a process (it uses no CPU until resumed).
pub fn suspend(pid: u32) -> bool {
    suspend_resume(pid, false)
}

/// Resumes a paused process. Harmless on one that isn't paused.
pub fn resume(pid: u32) -> bool {
    suspend_resume(pid, true)
}

/// The full command line a process was started with, to reopen it the same way.
pub fn command_line(pid: u32) -> Option<String> {
    type F = unsafe extern "system" fn(HANDLE, u32, *mut core::ffi::c_void, u32, *mut u32) -> i32;
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    let f = ntdll(b"NtQueryInformationProcess\0")?;
    // SAFETY: matches NtQueryInformationProcess's signature.
    let f: F = unsafe { std::mem::transmute(f) };
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut need = 0u32;
    unsafe { f(h.0, PROCESS_COMMAND_LINE_INFORMATION, std::ptr::null_mut(), 0, &mut need) };
    if need == 0 || need > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u64; (need as usize).div_ceil(8)];
    // SAFETY: buffer is `need` bytes; the result is a UNICODE_STRING pointing into the buffer.
    let status = unsafe { f(h.0, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr() as *mut _, need, &mut need) };
    if status < 0 {
        return None;
    }
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        _max: u16,
        buffer: *const u16,
    }
    let us = unsafe { &*(buf.as_ptr() as *const UnicodeString) };
    if us.buffer.is_null() {
        return None;
    }
    let s = unsafe { std::slice::from_raw_parts(us.buffer, us.length as usize / 2) };
    Some(String::from_utf16_lossy(s))
}

/// Asks a process to close the way clicking its window's X does (WM_CLOSE to its top-level
/// windows). Returns how many windows were asked. Apps may refuse or minimise to the tray.
pub fn ask_to_close(pid: u32) -> usize {
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, GW_OWNER, WM_CLOSE};
    struct Ctx {
        pid: u32,
        found: Vec<HWND>,
    }
    extern "system" fn each(hwnd: HWND, l: LPARAM) -> BOOL {
        // SAFETY: l is the &mut Ctx passed below, alive for the EnumWindows call.
        let ctx = unsafe { &mut *(l.0 as *mut Ctx) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let owned = unsafe { GetWindow(hwnd, GW_OWNER) }.map(|o| !o.0.is_null()).unwrap_or(false);
        if pid == ctx.pid && !owned && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            ctx.found.push(hwnd);
        }
        BOOL(1)
    }
    let mut ctx = Ctx { pid, found: Vec::new() };
    // SAFETY: the callback only touches ctx during the call.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Ctx as isize));
        for w in &ctx.found {
            let _ = PostMessageW(*w, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    ctx.found.len()
}

/// Starts a program with an exact command line (as the user, no console window).
pub fn launch(exe_path: &str, command_line: &str) -> Result<u32, String> {
    use windows::Win32::System::Threading::{CreateProcessW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW};
    let app: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut cmd: Vec<u16> = command_line.encode_utf16().chain(std::iter::once(0)).collect();
    let dir: Vec<u16> = std::path::Path::new(exe_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: all buffers are NUL-terminated and outlive the call; handles closed below.
    unsafe {
        CreateProcessW(
            PCWSTR(app.as_ptr()),
            PWSTR(cmd.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT,
            None,
            PCWSTR(dir.as_ptr()),
            &si,
            &mut pi,
        )
        .map_err(|e| e.to_string())?;
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    Ok(pi.dwProcessId)
}
