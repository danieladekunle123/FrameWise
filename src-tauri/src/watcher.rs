//! The game-profile watcher: `framewise.exe --watcher`, a small per-user background process that
//! starts with Windows while profiles are on.
//!
//! It sleeps until Windows reports a new foreground window. If that window's process belongs to a
//! game with an enabled profile, it saves the current state to the database, applies the profile
//! and waits on the game's processes; when the last one exits it puts everything back. It never
//! touches the game itself. If it crashed or the PC restarted mid-game, it restores the saved state
//! the next time it starts.
//!
//! It owns a tray icon (tooltip: its own RAM and CPU) with Open, Pause profiles and Quit watcher.
//! The app talks to it through the database plus two named events (reload, quit).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use framewise_checks::profiles::{self, ClosedApp, Game, PausedProcess, ProfileActions, SavedState};
use framewise_checks::tweak::{Tweak, TweakValue};
use rusqlite::Connection;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, OpenMutexW, SetEvent, EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE};

use crate::db;
use crate::procctl::{self, Handle};

const MUTEX: PCWSTR = w!("Local\\FrameWiseWatcher");
const RELOAD_EVENT: PCWSTR = w!("Local\\FrameWiseWatcherReload");
const QUIT_EVENT: PCWSTR = w!("Local\\FrameWiseWatcherQuit");
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "FrameWise watcher";

/// Design document budget while a game runs.
pub const BUDGET_RAM_MB: f64 = 15.0;
pub const BUDGET_CPU_PCT: f64 = 0.2;

pub const SETTING_ENABLED: &str = "profiles.enabled";
pub const SETTING_PAUSED: &str = "profiles.paused";
pub const SETTING_STATUS: &str = "watcher.status";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct WatcherStatus {
    pub pid: u32,
    pub started_at: u64,
    pub updated_at: u64,
    pub ram_mb: f64,
    pub cpu_pct: f64,
    pub peak_ram_mb: f64,
    pub paused: bool,
    pub active_game: Option<String>,
    pub profiles: usize,
    pub over_budget: bool,
}

// ---------------------------------------------------------------------------------------------
// Used by the app

pub fn is_running() -> bool {
    // SAFETY: handle closed straight away.
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, MUTEX) } {
        Ok(h) => {
            unsafe {
                let _ = CloseHandle(h);
            }
            true
        }
        Err(_) => false,
    }
}

fn signal(name: PCWSTR) {
    // SAFETY: handle closed straight away.
    if let Ok(h) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, name) } {
        unsafe {
            let _ = SetEvent(h);
            let _ = CloseHandle(h);
        }
    }
}

/// Tells a running watcher to reread profiles and settings.
pub fn reload() {
    signal(RELOAD_EVENT);
}

/// Asks the watcher to put everything back and exit.
pub fn quit() {
    signal(QUIT_EVENT);
}

/// Starts the watcher in the background (no window, independent of the app).
pub fn start() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if is_running() {
        reload();
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .arg("--watcher")
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the watcher: {e}"))
}

/// Starts the watcher when the user signs in (HKCU Run), or stops doing so.
pub fn set_autostart(on: bool) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE).map_err(|e| e.to_string())?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(RUN_VALUE, &format!("\"{}\" --watcher", exe.display())).map_err(|e| e.to_string())
    } else {
        match key.delete_value(RUN_VALUE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

pub fn autostart() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(RUN_VALUE))
        .is_ok()
}

pub fn pid_file() -> std::path::PathBuf {
    db::data_dir().join("watcher.pid")
}

// ---------------------------------------------------------------------------------------------
// The watcher process

thread_local! {
    /// Foreground-window PIDs reported by the WinEvent hook, handled by the main loop.
    static FOREGROUND: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
    /// Tray menu commands.
    static COMMANDS: RefCell<VecDeque<Command>> = const { RefCell::new(VecDeque::new()) };
}

#[derive(Clone, Copy, Debug)]
enum Command {
    Open,
    TogglePause,
    Quit,
    ReAddIcon,
}

struct Session {
    id: i64,
    game: Game,
    /// The game's processes: (pid, exit handle if Windows let us open one).
    pids: Vec<(u32, Option<Handle>)>,
    state: SavedState,
}

struct Watcher {
    db: Connection,
    profiles: Vec<(Game, ProfileActions)>,
    paused: bool,
    session: Option<Session>,
    own_session: u32,
    tray: Option<Tray>,
    started_at: u64,
    peak_ram_mb: f64,
    last_cpu: Option<(Instant, u64)>,
    last_status: WatcherStatus,
}

/// Entry point for `framewise.exe --watcher`.
pub fn watcher_main() -> i32 {
    // One watcher per user session.
    // SAFETY: the mutex handle lives for the whole process.
    let mutex = unsafe { CreateMutexW(None, true, MUTEX) };
    if mutex.is_err() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return 0;
    }
    let (Ok(reload_ev), Ok(quit_ev)) = (unsafe { CreateEventW(None, false, false, RELOAD_EVENT) }, unsafe { CreateEventW(None, true, false, QUIT_EVENT) }) else {
        return 2;
    };
    let Ok(conn) = db::open() else { return 3 };
    // Keep the watcher small: a quarter-megabyte page cache is plenty for a few rows a minute.
    let _ = conn.execute_batch("PRAGMA cache_size = -256;");
    let _ = std::fs::write(pid_file(), std::process::id().to_string());

    let mut w = Watcher {
        db: conn,
        profiles: Vec::new(),
        paused: false,
        session: None,
        own_session: procctl::session_of(std::process::id()).unwrap_or(0),
        tray: None,
        started_at: crate::now_secs(),
        peak_ram_mb: 0.0,
        last_cpu: None,
        last_status: WatcherStatus::default(),
    };

    // Crash recovery: anything a previous watcher changed and never put back.
    w.recover();
    w.load();
    w.tray = Tray::create().ok();
    let _hook = ForegroundHook::install();
    // A game that was already running when the watcher started.
    w.scan_running();
    w.sample_footprint();

    let mut next_sample = Instant::now() + Duration::from_secs(60);
    loop {
        // Wait for: quit, reload, any tracked game process exiting, a message (hook or tray), or
        // the next footprint sample. Games Windows won't let us open are checked every 5 s.
        let mut handles: Vec<HANDLE> = vec![quit_ev, reload_ev];
        let mut polled = false;
        if let Some(s) = &w.session {
            for (_, h) in &s.pids {
                match h {
                    Some(h) if handles.len() < 63 => handles.push(h.0),
                    _ => polled = true,
                }
            }
        }
        let until_sample = next_sample.saturating_duration_since(Instant::now());
        let timeout = if polled { until_sample.min(Duration::from_secs(5)) } else { until_sample };
        let r = wait(&handles, timeout);
        match r {
            Some(0) => break,
            Some(1) => w.load(),
            Some(i) if i < handles.len() => w.process_exited(handles[i]),
            _ => {}
        }
        pump_messages();
        let pids: Vec<u32> = FOREGROUND.with(|q| q.borrow_mut().drain(..).collect());
        for pid in pids {
            w.on_foreground(pid);
        }
        let cmds: Vec<Command> = COMMANDS.with(|q| q.borrow_mut().drain(..).collect());
        let mut quit = false;
        for c in cmds {
            match c {
                Command::Open => open_app(),
                Command::TogglePause => {
                    let p = !w.paused;
                    let _ = db::set_setting(&w.db, SETTING_PAUSED, if p { "1" } else { "0" });
                    w.load();
                }
                Command::Quit => quit = true,
                Command::ReAddIcon => {
                    if let Some(t) = &w.tray {
                        t.add();
                    }
                }
            }
        }
        if quit {
            // Quitting from the tray turns profiles off; the app shows that.
            let _ = db::set_setting(&w.db, SETTING_ENABLED, "0");
            let _ = set_autostart(false);
            break;
        }
        if polled {
            w.poll_untracked();
        }
        if Instant::now() >= next_sample {
            w.sample_footprint();
            next_sample = Instant::now() + Duration::from_secs(60);
        }
    }

    w.end_session("watcher stopped");
    let mut st = w.last_status.clone();
    st.pid = 0;
    let _ = db::set_setting(&w.db, SETTING_STATUS, &serde_json::to_string(&st).unwrap_or_default());
    let _ = std::fs::remove_file(pid_file());
    drop(w.tray.take());
    unsafe {
        let _ = CloseHandle(reload_ev);
        let _ = CloseHandle(quit_ev);
        if let Ok(m) = mutex {
            let _ = CloseHandle(m);
        }
    }
    0
}

/// Waits on handles and the message queue. Returns the signalled handle's index, or None for a
/// message or timeout.
fn wait(handles: &[HANDLE], timeout: Duration) -> Option<usize> {
    use windows::Win32::UI::WindowsAndMessaging::{MsgWaitForMultipleObjects, QS_ALLINPUT};
    // SAFETY: handles are valid for the call.
    let r = unsafe { MsgWaitForMultipleObjects(Some(handles), false, timeout.as_millis().min(u32::MAX as u128) as u32, QS_ALLINPUT) };
    let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
    (i < handles.len()).then_some(i)
}

fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
    let mut msg = MSG::default();
    // SAFETY: standard message pump; WinEvent and tray callbacks run inside it.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

impl Watcher {
    fn load(&mut self) {
        self.paused = db::setting(&self.db, SETTING_PAUSED).ok().flatten().as_deref() == Some("1");
        self.profiles = db::profiles(&self.db)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, p)| p.enabled)
            .map(|(g, p)| (g, p.actions.sanitised()))
            .filter(|(_, a)| !a.is_empty())
            .collect();
        // Pausing, or turning off the running game's profile, ends its session now.
        let keep = self.session.as_ref().is_some_and(|s| !self.paused && self.profiles.iter().any(|(g, _)| g.id == s.game.id));
        if !keep {
            self.end_session("profile paused or turned off");
        }
        if !self.paused {
            self.scan_running();
        }
        self.write_status();
    }

    /// Restores every session a previous watcher left open.
    fn recover(&mut self) {
        for s in db::open_sessions(&self.db).unwrap_or_default() {
            let mut state = s.state.clone();
            restore(&mut state);
            let _ = db::finish_session(&self.db, s.id, crate::now_secs(), &state);
        }
    }

    fn profile_for(&self, exe_path: &str) -> Option<(Game, ProfileActions)> {
        self.profiles.iter().find(|(g, _)| profiles::matches(g, exe_path)).cloned()
    }

    fn on_foreground(&mut self, pid: u32) {
        if self.paused || pid == std::process::id() {
            return;
        }
        let Some(path) = procctl::image_path(pid) else { return };
        if let Some(s) = &mut self.session {
            // Another process of the running game (a launcher handing over, for example).
            if profiles::matches(&s.game, &path) && !s.pids.iter().any(|(p, _)| *p == pid) {
                s.pids.push((pid, procctl::wait_handle(pid)));
            }
            return; // one profile at a time
        }
        if let Some((game, actions)) = self.profile_for(&path) {
            self.start_session(game, actions);
        }
    }

    /// Finds a profiled game that's already running (watcher start, or profiles just changed).
    fn scan_running(&mut self) {
        if self.session.is_some() || self.paused || self.profiles.is_empty() {
            return;
        }
        for (pid, _) in procctl::list() {
            if procctl::session_of(pid) != Some(self.own_session) {
                continue;
            }
            if let Some(path) = procctl::image_path(pid) {
                if let Some((game, actions)) = self.profile_for(&path) {
                    self.start_session(game, actions);
                    return;
                }
            }
        }
    }

    /// The game's processes now, in this session.
    fn game_pids(&self, game: &Game) -> Vec<u32> {
        procctl::list()
            .into_iter()
            .filter(|(pid, _)| procctl::session_of(*pid) == Some(self.own_session))
            .filter(|(pid, _)| procctl::image_path(*pid).is_some_and(|p| profiles::matches(game, &p)))
            .map(|(pid, _)| pid)
            .collect()
    }

    fn start_session(&mut self, game: Game, actions: ProfileActions) {
        let game_pids = self.game_pids(&game);
        if game_pids.is_empty() {
            return;
        }
        let mut state = SavedState { game_name: game.name.clone(), ..Default::default() };

        // Work out every change and save the previous state *before* changing anything.
        if let Some(plan) = &actions.power_plan {
            if let Ok(TweakValue::Text(now)) = framewise_checks::system::read(&Tweak::PowerScheme) {
                if !now.eq_ignore_ascii_case(plan) {
                    state.power_plan_before = Some(now);
                    state.power_plan_set = Some(plan.clone());
                }
            }
        }
        let mine: Vec<(u32, String)> = procctl::list()
            .into_iter()
            .filter(|(pid, _)| !game_pids.contains(pid) && *pid != std::process::id())
            .filter(|(pid, _)| procctl::session_of(*pid) == Some(self.own_session))
            .collect();
        let mut to_close: Vec<u32> = Vec::new();
        for (pid, exe) in &mine {
            let name = exe.to_lowercase();
            if !profiles::may_touch(&name) {
                continue;
            }
            if actions.pause.contains(&name) {
                if let Some(created) = procctl::creation_time(*pid) {
                    state.paused.push(PausedProcess { pid: *pid, exe: name.clone(), created });
                }
            } else if actions.close.contains(&name) {
                if let (Some(path), Some(cmd)) = (procctl::image_path(*pid), procctl::command_line(*pid)) {
                    let app = ClosedApp { exe_path: path, command_line: cmd };
                    if !state.closed.contains(&app) {
                        state.closed.push(app);
                    }
                    to_close.push(*pid);
                }
            }
        }
        let Ok(id) = db::insert_session(&self.db, &game.id, crate::now_secs(), &state) else { return };

        // Apply.
        if let Some(plan) = &state.power_plan_set {
            if framewise_checks::system::write(&Tweak::PowerScheme, &TweakValue::Text(plan.clone())).is_err() {
                state.power_plan_set = None;
                state.power_plan_before = None;
            }
        }
        state.paused.retain(|p| procctl::suspend(p.pid));
        for pid in &to_close {
            procctl::ask_to_close(*pid);
        }
        if !to_close.is_empty() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while to_close.iter().any(|p| procctl::is_running(*p)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(250));
            }
            // Apps that stayed open (many minimise to the tray instead of closing) aren't reopened.
            let still: Vec<String> = to_close
                .iter()
                .filter(|p| procctl::is_running(**p))
                .filter_map(|p| procctl::image_path(*p))
                .collect();
            state.not_closed = still.iter().map(|p| file_name(p)).collect();
            state.closed.retain(|c| !still.iter().any(|s| s.eq_ignore_ascii_case(&c.exe_path)));
        }
        let _ = db::update_session_state(&self.db, id, &state);

        let pids = game_pids.into_iter().map(|p| (p, procctl::wait_handle(p))).collect();
        self.session = Some(Session { id, game, pids, state });
        self.write_status();
    }

    fn process_exited(&mut self, handle: HANDLE) {
        if let Some(s) = &mut self.session {
            s.pids.retain(|(_, h)| h.as_ref().map(|h| h.0) != Some(handle));
        }
        self.check_session_end();
    }

    fn poll_untracked(&mut self) {
        if let Some(s) = &mut self.session {
            s.pids.retain(|(pid, h)| h.is_some() || procctl::is_running(*pid) || procctl::list().iter().any(|(p, _)| p == pid));
        }
        self.check_session_end();
    }

    /// Ends the session once none of the game's processes are left (a launcher may hand over to
    /// the real game, so look again before deciding).
    fn check_session_end(&mut self) {
        let Some(s) = &self.session else { return };
        if !s.pids.is_empty() {
            return;
        }
        let again = self.game_pids(&s.game);
        if again.is_empty() {
            self.end_session("game closed");
        } else if let Some(s) = &mut self.session {
            s.pids = again.into_iter().map(|p| (p, procctl::wait_handle(p))).collect();
        }
    }

    fn end_session(&mut self, _why: &str) {
        let Some(mut s) = self.session.take() else { return };
        restore(&mut s.state);
        let _ = db::finish_session(&self.db, s.id, crate::now_secs(), &s.state);
        self.write_status();
    }

    /// Measures the watcher's own private memory and CPU (the design document's budget) and logs it.
    fn sample_footprint(&mut self) {
        let pid = std::process::id();
        let Some(s) = framewise_checks::procmeter::sample(pid) else { return };
        let ram = s.private_ws_bytes as f64 / 1024.0 / 1024.0;
        let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
        let cpu = match self.last_cpu {
            Some((at, t)) => (s.cpu_time_100ns.saturating_sub(t)) as f64 / 1e7 / at.elapsed().as_secs_f64().max(0.001) / ncpu * 100.0,
            None => 0.0,
        };
        self.last_cpu = Some((Instant::now(), s.cpu_time_100ns));
        self.peak_ram_mb = self.peak_ram_mb.max(ram);
        let _ = db::insert_footprint(&self.db, "watcher", crate::now_secs(), ram, cpu);
        self.last_status.ram_mb = ram;
        self.last_status.cpu_pct = cpu;
        self.write_status();
    }

    fn write_status(&mut self) {
        let st = WatcherStatus {
            pid: std::process::id(),
            started_at: self.started_at,
            updated_at: crate::now_secs(),
            ram_mb: self.last_status.ram_mb,
            cpu_pct: self.last_status.cpu_pct,
            peak_ram_mb: self.peak_ram_mb,
            paused: self.paused,
            active_game: self.session.as_ref().map(|s| s.game.name.clone()),
            profiles: self.profiles.len(),
            over_budget: self.last_status.ram_mb > BUDGET_RAM_MB || self.last_status.cpu_pct > BUDGET_CPU_PCT,
        };
        let _ = db::set_setting(&self.db, SETTING_STATUS, &serde_json::to_string(&st).unwrap_or_default());
        if let Some(t) = &self.tray {
            let what = if self.paused {
                "profiles paused".to_string()
            } else if let Some(g) = &st.active_game {
                format!("profile on: {g}")
            } else {
                format!("{} profile{} ready", st.profiles, if st.profiles == 1 { "" } else { "s" })
            };
            t.set_tooltip(&format!("FrameWise watcher: {:.1} MB RAM · {:.2}% CPU\n{what}", st.ram_mb, st.cpu_pct), self.paused);
        }
        self.last_status = st;
    }
}

fn file_name(p: &str) -> String {
    p.rsplit(['\\', '/']).next().unwrap_or(p).to_lowercase()
}

/// Puts back everything a session changed. Safe to run twice and after a restart: each step checks
/// the current state first.
fn restore(state: &mut SavedState) {
    if let (Some(before), Ok(TweakValue::Text(now))) = (&state.power_plan_before, framewise_checks::system::read(&Tweak::PowerScheme)) {
        if profiles::should_restore_plan(state, Some(&now)) {
            let _ = framewise_checks::system::write(&Tweak::PowerScheme, &TweakValue::Text(before.clone()));
        }
    }
    for p in &state.paused {
        // Same PID *and* start time, so a reused PID is never touched.
        if procctl::creation_time(p.pid) == Some(p.created) {
            procctl::resume(p.pid);
        }
    }
    let running: Vec<String> = procctl::list().into_iter().filter_map(|(pid, _)| procctl::image_path(pid)).collect();
    for c in &state.closed {
        if !running.iter().any(|r| r.eq_ignore_ascii_case(&c.exe_path)) {
            let _ = procctl::launch(&c.exe_path, &c.command_line);
        }
    }
}

fn open_app() {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};
    // SAFETY: plain window calls.
    unsafe {
        if let Ok(h) = FindWindowW(None, w!("FrameWise")) {
            if !h.0.is_null() {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
                return;
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
}

// ---------------------------------------------------------------------------------------------
// Foreground-window hook

struct ForegroundHook(windows::Win32::UI::Accessibility::HWINEVENTHOOK);

impl ForegroundHook {
    fn install() -> Option<ForegroundHook> {
        use windows::Win32::UI::Accessibility::SetWinEventHook;
        use windows::Win32::UI::WindowsAndMessaging::{EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS};
        extern "system" fn on_event(
            _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
            _event: u32,
            hwnd: HWND,
            _obj: i32,
            _child: i32,
            _thread: u32,
            _time: u32,
        ) {
            use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
            let mut pid = 0u32;
            // SAFETY: plain query on the reported window.
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 {
                FOREGROUND.with(|q| {
                    let mut q = q.borrow_mut();
                    if q.len() < 64 {
                        q.push_back(pid);
                    }
                });
            }
        }
        // SAFETY: out-of-context hook, delivered through this thread's message loop.
        let h = unsafe { SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS) };
        (!h.0.is_null()).then_some(ForegroundHook(h))
    }
}

impl Drop for ForegroundHook {
    fn drop(&mut self) {
        // SAFETY: our own hook.
        unsafe {
            let _ = windows::Win32::UI::Accessibility::UnhookWinEvent(self.0);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tray icon

const WM_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
const ID_OPEN: usize = 1;
const ID_PAUSE: usize = 2;
const ID_QUIT: usize = 3;

thread_local! {
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static PAUSED_LABEL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct Tray {
    hwnd: HWND,
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    tip: RefCell<String>,
}

impl Tray {
    fn create() -> Result<Tray, String> {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, LoadIconW, RegisterClassW, RegisterWindowMessageW, IDI_APPLICATION, WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPED,
        };
        // SAFETY: standard window creation; the window lives as long as the Tray.
        unsafe {
            let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = w!("FrameWiseWatcherTray");
            let wc = WNDCLASSW { lpfnWndProc: Some(tray_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("FrameWise watcher"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, inst, None)
                .map_err(|e| e.to_string())?;
            TASKBAR_CREATED.with(|c| c.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
            // The app icon embedded by Tauri; Windows' generic icon if it isn't found.
            let icon = [32512u16, 1]
                .iter()
                .find_map(|id| LoadIconW(inst, PCWSTR(*id as usize as *const u16)).ok())
                .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
                .unwrap_or_default();
            let t = Tray { hwnd, icon, tip: RefCell::new("FrameWise watcher".into()) };
            t.add();
            Ok(t)
        }
    }

    fn data(&self) -> windows::Win32::UI::Shell::NOTIFYICONDATAW {
        use windows::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NOTIFYICONDATAW};
        let mut d = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        for (i, c) in self.tip.borrow().encode_utf16().take(127).enumerate() {
            d.szTip[i] = c;
        }
        d
    }

    fn add(&self) {
        use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIM_ADD, NIM_SETVERSION, NOTIFYICON_VERSION_4};
        let mut d = self.data();
        // SAFETY: d is fully initialised.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &d);
            d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
        }
    }

    fn set_tooltip(&self, tip: &str, paused: bool) {
        use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIM_MODIFY};
        PAUSED_LABEL.with(|p| p.set(paused));
        *self.tip.borrow_mut() = tip.to_string();
        let d = self.data();
        // SAFETY: d is fully initialised.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIM_DELETE};
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        let d = self.data();
        // SAFETY: removing our own icon and window.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &d);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

extern "system" fn tray_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow, TrackPopupMenu, MF_SEPARATOR, MF_STRING,
        TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
    };
    let push = |c: Command| COMMANDS.with(|q| q.borrow_mut().push_back(c));
    if msg == TASKBAR_CREATED.with(|c| c.get()) && msg != 0 {
        push(Command::ReAddIcon);
        return LRESULT(0);
    }
    if msg == WM_TRAY {
        // NOTIFYICON_VERSION_4: the event is in the low word of lParam.
        let event = (lp.0 as u32) & 0xFFFF;
        match event {
            e if e == WM_LBUTTONUP || e == WM_LBUTTONDBLCLK => push(Command::Open),
            e if e == WM_RBUTTONUP || e == WM_CONTEXTMENU => {
                // SAFETY: standard popup-menu sequence (SetForegroundWindow first so it closes properly).
                unsafe {
                    let Ok(menu) = CreatePopupMenu() else { return LRESULT(0) };
                    let paused = PAUSED_LABEL.with(|p| p.get());
                    let _ = AppendMenuW(menu, MF_STRING, ID_OPEN, w!("Open FrameWise"));
                    let _ = AppendMenuW(menu, MF_STRING, ID_PAUSE, if paused { w!("Resume profiles") } else { w!("Pause profiles") });
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                    let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit watcher (turns profiles off)"));
                    let mut pt = Default::default();
                    let _ = GetCursorPos(&mut pt);
                    let _ = SetForegroundWindow(hwnd);
                    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
                    let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
                    let _ = DestroyMenu(menu);
                    match cmd.0 as usize {
                        ID_OPEN => push(Command::Open),
                        ID_PAUSE => push(Command::TogglePause),
                        ID_QUIT => push(Command::Quit),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        return LRESULT(0);
    }
    // SAFETY: default handling for everything else.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}
