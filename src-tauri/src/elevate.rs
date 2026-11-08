//! The elevated helper: the same FrameWise executable started with `--helper` through one UAC
//! prompt per task. It accepts only the typed, allowlisted requests in
//! [`framewise_checks::tweak::HelperRequest`], refuses anything else, and exits when the task ends
//! (or after 15 minutes at most).

use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use framewise_checks::cleanup::{self, CleanCategory};
use framewise_checks::tweak::{validate_elevated, HelperReply, HelperRequest};
use framewise_checks::{system, tweak};
use serde_json::json;

use crate::pipe::{Owned, Server};

/// Error shown to the interface, with a machine-readable code.
#[derive(serde::Serialize, Debug, Clone)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        ApiError { code: code.into(), message: message.into() }
    }
}

impl From<String> for ApiError {
    fn from(message: String) -> Self {
        ApiError::new("error", message)
    }
}

/// A running elevated helper, owned by one task.
pub struct Elevated {
    server: Server,
    process: Owned,
}

impl Elevated {
    /// Starts the helper. Windows shows the UAC prompt; `hwnd` is the FrameWise window it belongs to.
    pub fn start(hwnd: isize) -> Result<Elevated, ApiError> {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
        use windows::Win32::System::Threading::GetProcessId;
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let name = crate::pipe::random_name();
        let server = Server::create(&name)?;
        let exe = std::env::current_exe().map_err(|e| ApiError::new("error", format!("Couldn't find FrameWise's own program file: {e}")))?;
        let verb = HSTRING::from("runas");
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(format!("--helper --pipe \"{name}\" --parent {}", std::process::id()));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            hwnd: HWND(hwnd as *mut _),
            lpVerb: windows::core::PCWSTR(verb.as_ptr()),
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: all strings outlive the call; hProcess is closed by Owned.
        if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
            if e.code() == ERROR_CANCELLED.to_hresult() {
                return Err(ApiError::new(
                    "elevation_cancelled",
                    "Windows asked for permission and it wasn't given, so nothing was changed.",
                ));
            }
            return Err(ApiError::new("elevation_failed", format!("Couldn't start the FrameWise helper: {e}")));
        }
        let process = Owned(info.hProcess);
        // SAFETY: valid process handle.
        let pid = unsafe { GetProcessId(process.0) };
        server.accept(process.0, pid, Duration::from_secs(30)).map_err(|e| ApiError::new("helper_failed", e))?;
        Ok(Elevated { server, process })
    }

    pub fn call(&mut self, req: &HelperRequest, timeout: Duration) -> Result<serde_json::Value, ApiError> {
        let line = serde_json::to_string(req).map_err(|e| e.to_string())?;
        self.server.write_line(&line, self.process.0).map_err(|e| ApiError::new("helper_failed", e))?;
        let reply = self.server.read_line(self.process.0, timeout).map_err(|e| ApiError::new("helper_failed", e))?;
        let reply: HelperReply =
            serde_json::from_str(&reply).map_err(|_| ApiError::new("helper_failed", "The helper sent an unreadable reply."))?;
        if reply.ok {
            Ok(reply.data)
        } else {
            Err(ApiError::new(
                reply.code.as_deref().unwrap_or("helper_error"),
                reply.error.unwrap_or_else(|| "The helper reported an error.".into()),
            ))
        }
    }
}

impl Drop for Elevated {
    fn drop(&mut self) {
        let _ = self.server.write_line(&serde_json::to_string(&HelperRequest::Done).unwrap_or_default(), self.process.0);
        // SAFETY: valid process handle; give the helper a moment to exit cleanly.
        unsafe {
            let _ = windows::Win32::System::Threading::WaitForSingleObject(self.process.0, 3000);
        }
    }
}

/// Debug builds: starts the helper *without* elevation and checks the channel and the allowlist.
/// Nothing is written: the only requests that would change anything are ones the helper must refuse.
#[cfg(debug_assertions)]
pub fn selftest() -> i32 {
    use framewise_checks::tweak::{StartupSource, Tweak, TweakValue};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE};
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };
    let name = crate::pipe::random_name();
    let server = match Server::create(&name) {
        Ok(s) => s,
        Err(e) => {
            println!("FAIL create pipe: {e}");
            return 1;
        }
    };
    // A second server on the same name must fail (first-instance only).
    check("pipe name can't be taken over", Server::create(&name).is_err(), "second CreateNamedPipe refused".into());
    let exe = std::env::current_exe().expect("exe");
    let child = std::process::Command::new(&exe)
        .args(["--helper", "--pipe", &name, "--parent", &std::process::id().to_string()])
        .spawn()
        .expect("spawn helper");
    // SAFETY: handle closed by Owned.
    let process = Owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, child.id()) }.expect("open"));
    if let Err(e) = server.accept(process.0, child.id(), Duration::from_secs(10)) {
        println!("FAIL helper connects: {e}");
        return 1;
    }
    check("helper connects and passes the PID check", true, format!("pid {}", child.id()));
    let mut h = Elevated { server, process };

    let r = h.call(&HelperRequest::ScanCleanup { categories: vec![CleanCategory::WindowsTemp, CleanCategory::UserTemp] }, Duration::from_secs(60));
    let n = r.as_ref().ok().and_then(|v| v.as_array().map(|a| a.len()));
    check("scan request answered (per-user category filtered out)", n == Some(1), format!("{n:?}"));

    let r = h.call(&HelperRequest::Write { tweak: Tweak::GameMode, value: TweakValue::Dword(1) }, Duration::from_secs(10));
    check("per-user setting refused by helper", matches!(&r, Err(e) if e.code == "refused"), format!("{r:?}"));

    let r = h.call(&HelperRequest::Write { tweak: Tweak::Hags, value: TweakValue::Dword(7) }, Duration::from_secs(10));
    check("invalid value refused", matches!(&r, Err(e) if e.code == "refused"), format!("{r:?}"));

    let sneaky = Tweak::StartupEntry { source: StartupSource::MachineFolder, name: r"..\..\evil.lnk".into() };
    let r = h.call(&HelperRequest::Write { tweak: sneaky, value: TweakValue::Absent }, Duration::from_secs(10));
    check("path tricks in names refused", matches!(&r, Err(e) if e.code == "refused"), format!("{r:?}"));

    let r = h.call(&HelperRequest::Clean { category: CleanCategory::UserTemp }, Duration::from_secs(10));
    check("per-user cleanup refused by helper", matches!(&r, Err(e) if e.code == "refused"), format!("{r:?}"));

    let r = h.call(&HelperRequest::RemoveDriverPackage { inf: "nvlddmkm.sys".into() }, Duration::from_secs(10));
    check("non-package driver name refused", matches!(&r, Err(e) if e.code == "refused"), format!("{r:?}"));

    let raw = h.server.write_line(r#"{"op":"run","cmd":"calc.exe"}"#, h.process.0).and_then(|_| h.server.read_line(h.process.0, Duration::from_secs(10)));
    check("unknown request refused", raw.as_deref().is_ok_and(|l| l.contains("\"refused\"")), format!("{raw:?}"));

    drop(h);
    let mut child = child;
    let exited = (0..30).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        matches!(child.try_wait(), Ok(Some(_)))
    });
    check("helper exits when the task ends", exited, String::new());
    if failures == 0 {
        println!("All helper self-tests passed.");
    }
    failures
}

// ---------------------------------------------------------------------------------------------
// Helper process

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// Entry point for `framewise.exe --helper --pipe <name> --parent <pid>`.
pub fn helper_main(args: &[String]) -> i32 {
    let (Some(pipe), Some(parent)) = (arg_after(args, "--pipe"), arg_after(args, "--parent").and_then(|p| p.parse::<u32>().ok())) else {
        return 2;
    };
    // Never outlive a stuck task: exit after 15 minutes without a request. (A monitoring session
    // polls several times a second, so it keeps the helper alive while it runs.)
    let last_request = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(crate::now_secs()));
    {
        let last = last_request.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(10));
            if crate::now_secs().saturating_sub(last.load(std::sync::atomic::Ordering::Relaxed)) > 15 * 60 {
                std::process::exit(3);
            }
        });
    }
    let Ok(file) = crate::pipe::connect_client(&pipe, parent) else {
        return 4;
    };
    // COM for WMI and System Restore, once for the helper's lifetime.
    let _com = wmi::COMLibrary::new();
    let mut writer = match file.try_clone() {
        Ok(w) => w,
        Err(_) => return 5,
    };
    let reader = BufReader::new(file);
    let mut ctx = HelperCtx { parent, capture: None };
    for line in reader.lines() {
        let Ok(line) = line else { break };
        last_request.store(crate::now_secs(), std::sync::atomic::Ordering::Relaxed);
        if line.len() > 64 * 1024 {
            break;
        }
        let req = serde_json::from_str::<HelperRequest>(&line);
        let done = matches!(req, Ok(HelperRequest::Done));
        let reply = match req {
            Ok(r) => handle(r, &mut ctx),
            Err(_) => HelperReply::err("refused", "The helper only accepts FrameWise's own requests."),
        };
        let out = serde_json::to_string(&reply).unwrap_or_else(|_| r#"{"ok":false,"error":"encoding"}"#.into());
        if writeln!(writer, "{out}").and_then(|_| writer.flush()).is_err() || done {
            break;
        }
    }
    0
}

struct HelperCtx {
    /// The FrameWise process that started this helper (already verified when connecting).
    parent: u32,
    capture: Option<crate::capture::PresentMon>,
}

fn handle(req: HelperRequest, ctx: &mut HelperCtx) -> HelperReply {
    match req {
        HelperRequest::Done => {
            ctx.capture = None;
            HelperReply::ok(json!(null))
        }
        HelperRequest::StartCapture => {
            ctx.capture = None;
            match crate::capture::PresentMon::start() {
                Ok(pm) => {
                    ctx.capture = Some(pm);
                    HelperReply::ok(json!(null))
                }
                Err(e) => HelperReply::err("capture_failed", e),
            }
        }
        HelperRequest::ReadCapture { max_lines } => match ctx.capture.as_mut() {
            None => HelperReply::err("capture_stopped", "No capture is running."),
            Some(pm) => {
                let lines = crate::capture::drain(&pm.lines, max_lines.min(5000) as usize);
                if lines.is_empty() {
                    if let Some(why) = pm.exited() {
                        ctx.capture = None;
                        return HelperReply::err("capture_stopped", why);
                    }
                }
                HelperReply::ok(json!(lines))
            }
        },
        HelperRequest::StopCapture => {
            ctx.capture = None;
            HelperReply::ok(json!(null))
        }
        HelperRequest::AllowMonitoringWithoutPrompt => match crate::capture::add_parent_user_to_perf_log_users(ctx.parent) {
            Ok(()) => HelperReply::ok(json!(null)),
            Err(e) => HelperReply::err("group_failed", e),
        },
        HelperRequest::CreateRestorePoint { description } => {
            let clean: String = description.chars().filter(|c| !c.is_control()).take(64).collect();
            match system::create_restore_point(&clean) {
                Ok(info) => HelperReply::ok(json!(info)),
                Err((code, msg)) => HelperReply::err(&code, msg),
            }
        }
        HelperRequest::Write { tweak, value } => {
            if let Err(e) = validate_elevated(&tweak, &value) {
                return HelperReply::err("refused", e);
            }
            if let Err(e) = system::write(&tweak, &value) {
                return HelperReply::err("write_failed", e);
            }
            match system::read(&tweak) {
                Ok(v) => HelperReply::ok(json!(v)),
                Err(e) => HelperReply::err("verify_failed", e),
            }
        }
        HelperRequest::ScanCleanup { categories } => {
            let scans: Vec<_> = categories
                .into_iter()
                .filter(|c| c.info().needs_admin && *c != CleanCategory::DriverPackages)
                .map(cleanup::scan)
                .collect();
            HelperReply::ok(json!(scans))
        }
        HelperRequest::Clean { category } => {
            if !category.info().needs_admin || category == CleanCategory::DriverPackages {
                return HelperReply::err("refused", "That category is cleaned without admin rights.");
            }
            HelperReply::ok(json!(cleanup::clean(category, false)))
        }
        HelperRequest::RemoveDriverPackage { inf } => {
            if !tweak::valid_oem_inf(&inf) {
                return HelperReply::err("refused", "Not a driver store package name.");
            }
            match cleanup::remove_driver_package(&inf) {
                Ok(bytes) => HelperReply::ok(json!(bytes)),
                Err(e) => HelperReply::err("remove_failed", e),
            }
        }
    }
}
