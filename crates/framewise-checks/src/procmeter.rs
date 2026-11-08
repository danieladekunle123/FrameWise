//! Measures a process the way Task Manager does: private working set and CPU time.
//!
//! Shared by every FrameWise process so each can report its own footprint.

use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

#[derive(Clone, Copy, Debug)]
pub struct ProcSample {
    /// Private working set in bytes (Task Manager's "Memory" column).
    pub private_ws_bytes: u64,
    /// Total kernel + user CPU time, in 100 ns units.
    pub cpu_time_100ns: u64,
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

pub fn sample(pid: u32) -> Option<ProcSample> {
    // SAFETY: plain Win32 calls on a handle we open and close here; out pointers are valid.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut mem = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            ..Default::default()
        };
        let mem_ok = GetProcessMemoryInfo(
            h,
            &mut mem as *mut PROCESS_MEMORY_COUNTERS_EX2 as *mut PROCESS_MEMORY_COUNTERS,
            mem.cb,
        )
        .is_ok();
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let times_ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
        let _ = CloseHandle(h);
        if !mem_ok || !times_ok {
            return None;
        }
        Some(ProcSample { private_ws_bytes: mem.PrivateWorkingSetSize as u64, cpu_time_100ns: ft(k) + ft(u) })
    }
}
