//! Frame-time capture with the bundled PresentMon.
//!
//! PresentMon reads Windows' graphics event trace, which needs admin rights or membership of the
//! "Performance Log Users" group. When the user already has that, FrameWise runs PresentMon
//! itself; otherwise the elevated helper runs it (one UAC prompt per monitoring session) and
//! passes its output back over the helper pipe. Either way:
//!
//! * the executable must match the pinned SHA-256 (`tweak::PRESENTMON_SHA256`);
//! * the arguments are fixed (`tweak::PRESENTMON_ARGS`);
//! * PresentMon runs inside a kill-on-close job, so it can never outlive FrameWise or the helper.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use framewise_checks::tweak::{HelperRequest, PRESENTMON_ARGS, PRESENTMON_SHA256};
use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{CloseHandle, HANDLE};

use crate::elevate::{ApiError, Elevated};

/// Most lines kept unread before the oldest are dropped (about a minute at 1000 FPS).
const MAX_BUFFERED: usize = 60_000;

/// Where the bundled PresentMon is, checked against its pinned hash.
pub fn presentmon_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no exe folder")?.to_path_buf();
    let mut candidates = vec![dir.join("presentmon.exe"), dir.join("resources").join("presentmon.exe")];
    // Development builds: the copy in the repository.
    if cfg!(debug_assertions) {
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/presentmon/PresentMon-2.6.0-x64.exe"));
    }
    let path = candidates.into_iter().find(|p| p.is_file()).ok_or("PresentMon isn't installed with FrameWise. Reinstall FrameWise to restore it.")?;
    verify(&path)?;
    Ok(path)
}

fn verify(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read PresentMon: {e}"))?;
    let hash: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if hash != PRESENTMON_SHA256 {
        return Err("The bundled PresentMon has been changed, so FrameWise won't run it. Reinstall FrameWise to restore it.".into());
    }
    Ok(())
}

/// Whether this process may start an ETW trace itself: it's elevated, or the user is in
/// "Performance Log Users" (S-1-5-32-559) as of this sign-in.
pub fn can_capture_directly() -> bool {
    use windows::Win32::Security::{CheckTokenMembership, CreateWellKnownSid, WinBuiltinPerfLoggingUsersSid, PSID};
    if is_elevated() {
        return true;
    }
    let mut sid = [0u8; 68];
    let mut len = sid.len() as u32;
    let psid = PSID(sid.as_mut_ptr() as *mut _);
    // SAFETY: sid buffer is large enough for any well-known SID (SECURITY_MAX_SID_SIZE = 68).
    if unsafe { CreateWellKnownSid(WinBuiltinPerfLoggingUsersSid, None, psid, &mut len) }.is_err() {
        return false;
    }
    let mut member = windows::Win32::Foundation::BOOL(0);
    // SAFETY: psid is valid; None = the calling thread's effective token.
    unsafe { CheckTokenMembership(None, psid, &mut member) }.is_ok() && member.as_bool()
}

pub fn is_elevated() -> bool {
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token = HANDLE::default();
    // SAFETY: pseudo-handle for this process; token closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let mut e = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    // SAFETY: e is a TOKEN_ELEVATION and the size matches.
    let ok = unsafe { GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len) }.is_ok();
    unsafe {
        let _ = CloseHandle(token);
    }
    ok && e.TokenIsElevated != 0
}

/// A job object that kills everything in it when the last handle closes.
pub struct KillOnClose(HANDLE);
unsafe impl Send for KillOnClose {}

impl KillOnClose {
    pub fn new() -> Result<KillOnClose, String> {
        use windows::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: anonymous job; handle owned by the struct.
        let job = unsafe { CreateJobObjectW(None, None) }.map_err(|e| e.to_string())?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: info is the struct the class expects.
        unsafe {
            SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of_val(&info) as u32)
        }
        .map_err(|e| e.to_string())?;
        Ok(KillOnClose(job))
    }

    pub fn add(&self, child: &Child) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        // SAFETY: both handles are valid for the call.
        unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }.map_err(|e| e.to_string())
    }
}

impl Drop for KillOnClose {
    fn drop(&mut self) {
        // SAFETY: our own handle, closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Unread output lines, filled by a reader thread.
pub type Lines = Arc<Mutex<VecDeque<String>>>;

pub fn drain(lines: &Lines, max: usize) -> Vec<String> {
    let Ok(mut q) = lines.lock() else { return Vec::new() };
    let n = q.len().min(max);
    q.drain(..n).collect()
}

/// A running PresentMon with its output pump. Used by both the core and the helper.
pub struct PresentMon {
    child: Child,
    _job: KillOnClose,
    pub lines: Lines,
    pub errors: Lines,
}

impl PresentMon {
    pub fn start() -> Result<PresentMon, String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let path = presentmon_path()?;
        let job = KillOnClose::new()?;
        let mut child = Command::new(&path)
            .args(PRESENTMON_ARGS)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Couldn't start PresentMon: {e}"))?;
        if let Err(e) = job.add(&child) {
            let _ = child.kill();
            return Err(format!("Couldn't contain PresentMon: {e}"));
        }
        let lines: Lines = Default::default();
        let errors: Lines = Default::default();
        pump(child.stdout.take(), lines.clone(), MAX_BUFFERED);
        pump(child.stderr.take(), errors.clone(), 200);
        Ok(PresentMon { child, _job: job, lines, errors })
    }

    /// None while running; otherwise why it stopped.
    pub fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                let err = self.errors.lock().map(|q| q.iter().cloned().collect::<Vec<_>>().join(" ")).unwrap_or_default();
                Some(if err.contains("access denied") {
                    "access_denied".into()
                } else {
                    format!("PresentMon stopped ({status}). {}", err.chars().take(300).collect::<String>())
                })
            }
            Err(e) => Some(e.to_string()),
        }
    }
}

impl Drop for PresentMon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Killing PresentMon leaves its Windows trace session running until something stops it, so
        // stop it now (the next start would also clear it, with --stop_existing_session).
        stop_trace_session();
    }
}

fn stop_trace_session() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(path) = presentmon_path() else { return };
    if let Ok(mut c) = Command::new(path)
        .args(["--terminate_existing_session", "--session_name", "FrameWise"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        for _ in 0..30 {
            if matches!(c.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = c.kill();
    }
}

fn pump(stream: Option<impl std::io::Read + Send + 'static>, into: Lines, cap: usize) {
    let Some(stream) = stream else { return };
    let _ = std::thread::Builder::new().name("presentmon-out".into()).spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if let Ok(mut q) = into.lock() {
                if q.len() >= cap {
                    q.pop_front();
                }
                q.push_back(line);
            }
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Direct,
    Helper,
}

/// A capture from the core's point of view.
pub enum Capture {
    Direct(PresentMon),
    Helper(Elevated),
}

impl Capture {
    /// Starts capture, directly if Windows allows it, otherwise through the helper (UAC prompt).
    pub fn start(hwnd: isize) -> Result<(Capture, Mode), ApiError> {
        if can_capture_directly() {
            let mut pm = PresentMon::start().map_err(|e| ApiError::new("capture_failed", e))?;
            // A refused trace session exits straight away.
            std::thread::sleep(Duration::from_millis(600));
            match pm.exited() {
                None => return Ok((Capture::Direct(pm), Mode::Direct)),
                Some(r) if r == "access_denied" => {} // fall through to the helper
                Some(r) => return Err(ApiError::new("capture_failed", r)),
            }
        }
        let mut h = Elevated::start(hwnd)?;
        h.call(&HelperRequest::StartCapture, Duration::from_secs(20))?;
        Ok((Capture::Helper(h), Mode::Helper))
    }

    /// Output lines since the last read.
    pub fn read(&mut self, max: usize) -> Result<Vec<String>, ApiError> {
        match self {
            Capture::Direct(pm) => {
                let lines = drain(&pm.lines, max);
                if lines.is_empty() {
                    if let Some(why) = pm.exited() {
                        return Err(ApiError::new("capture_stopped", why));
                    }
                }
                Ok(lines)
            }
            Capture::Helper(h) => {
                let v = h.call(&HelperRequest::ReadCapture { max_lines: max as u32 }, Duration::from_secs(10))?;
                Ok(serde_json::from_value(v).unwrap_or_default())
            }
        }
    }

}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Capture::Helper(h) = self {
            let _ = h.call(&HelperRequest::StopCapture, Duration::from_secs(5));
        }
    }
}

/// Helper side: adds the parent process's user to "Performance Log Users".
pub fn add_parent_user_to_perf_log_users(parent_pid: u32) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::NetworkManagement::NetManagement::{NetLocalGroupAddMembers, LOCALGROUP_MEMBERS_INFO_0};
    use windows::Win32::Security::{
        CreateWellKnownSid, GetTokenInformation, LookupAccountSidW, TokenUser, WinBuiltinPerfLoggingUsersSid, PSID, SID_NAME_USE, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

    // The user who started the helper: from the parent's token, not from any request.
    // SAFETY: handles are closed below; buffers are sized from the API's answers.
    unsafe {
        let proc_ = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, parent_pid).map_err(|e| e.to_string())?;
        let mut token = HANDLE::default();
        let r = OpenProcessToken(proc_, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(proc_);
        r.map_err(|e| e.to_string())?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let r = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut _), len, &mut len);
        let _ = CloseHandle(token);
        r.map_err(|e| e.to_string())?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);

        // The group's name in this Windows language.
        let mut gsid = [0u8; 68];
        let mut glen = gsid.len() as u32;
        let gpsid = PSID(gsid.as_mut_ptr() as *mut _);
        CreateWellKnownSid(WinBuiltinPerfLoggingUsersSid, None, gpsid, &mut glen).map_err(|e| e.to_string())?;
        let mut name = vec![0u16; 256];
        let mut name_len = name.len() as u32;
        let mut domain = vec![0u16; 256];
        let mut domain_len = domain.len() as u32;
        let mut use_ = SID_NAME_USE::default();
        LookupAccountSidW(None, gpsid, PWSTR(name.as_mut_ptr()), &mut name_len, PWSTR(domain.as_mut_ptr()), &mut domain_len, &mut use_)
            .map_err(|e| e.to_string())?;

        let member = LOCALGROUP_MEMBERS_INFO_0 { lgrmi0_sid: user.User.Sid };
        let r = NetLocalGroupAddMembers(None, PWSTR(name.as_mut_ptr()), 0, &member as *const _ as *const u8, 1);
        // 1378 = ERROR_MEMBER_IN_ALIAS: already a member, which is fine.
        if r == 0 || r == 1378 {
            Ok(())
        } else {
            Err(format!("Windows couldn't add you to the group (error {r})."))
        }
    }
}
