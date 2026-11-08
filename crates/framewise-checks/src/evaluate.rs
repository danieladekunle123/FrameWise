//! The optimisation catalogue: 16 checks, each a pure function of a [`Snapshot`].

use crate::model::*;
use crate::parse::*;
use crate::snapshot::*;

/// Every check, in catalogue order (typical impact, highest first).
pub const CHECK_IDS: &[&str] = &[
    "ram_speed",
    "refresh_rate",
    "ram_channels",
    "gpu_driver",
    "thermal_throttling",
    "game_on_hdd",
    "overlays",
    "startup_bloat",
    "power_plan",
    "game_mode",
    "hags",
    "windowed_optimizations",
    "memory_integrity",
    "variable_refresh",
    "system_drive_space",
    "network_latency",
];

pub fn evaluate_all(s: &Snapshot) -> Vec<Finding> {
    CHECK_IDS.iter().filter_map(|id| evaluate_one(s, id)).collect()
}

pub fn evaluate_one(s: &Snapshot, id: &str) -> Option<Finding> {
    let mut f = evaluate_raw(s, id)?;
    f.fix = crate::fixes::fix_info(&f);
    Some(f)
}

fn evaluate_raw(s: &Snapshot, id: &str) -> Option<Finding> {
    Some(match id {
        "ram_speed" => ram_speed(s),
        "refresh_rate" => refresh_rate(s),
        "ram_channels" => ram_channels(s),
        "gpu_driver" => gpu_driver(s),
        "thermal_throttling" => thermal_throttling(s),
        "game_on_hdd" => game_on_hdd(s),
        "overlays" => overlays(s),
        "startup_bloat" => startup_bloat(s),
        "power_plan" => power_plan(s),
        "game_mode" => game_mode(s),
        "hags" => hags(s),
        "windowed_optimizations" => windowed_optimizations(s),
        "memory_integrity" => memory_integrity(s),
        "variable_refresh" => variable_refresh(s),
        "system_drive_space" => system_drive_space(s),
        "network_latency" => network_latency(s),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// Helpers

fn s(v: &str) -> String {
    v.to_string()
}

fn steps(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn link(label: &str, url: &str) -> Link {
    Link { label: s(label), url: s(url) }
}

struct Meta {
    id: &'static str,
    title: &'static str,
    impact: Impact,
    action: Action,
    explanation: &'static str,
    fix_changes: &'static str,
    one_click_phase: Option<u8>,
}

fn finding(m: Meta, guide: Guide) -> Finding {
    Finding {
        check_id: s(m.id),
        title: s(m.title),
        status: Status::Unknown,
        impact: m.impact,
        action: m.action,
        summary: String::new(),
        explanation: s(m.explanation),
        fix_changes: s(m.fix_changes),
        detected: None,
        expected: None,
        details: Vec::new(),
        guide,
        one_click_phase: m.one_click_phase,
        fix: None,
    }
}

fn set(mut f: Finding, status: Status, summary: impl Into<String>) -> Finding {
    f.status = status;
    f.summary = summary.into();
    f
}

fn couldnt_read(f: Finding, what: &str) -> Finding {
    set(f, Status::Unknown, format!("Couldn't read {what}. Use the steps below to check by hand."))
}

fn is_real_gpu(g: &GpuInfo) -> bool {
    let id = g.pnp_device_id.to_ascii_uppercase();
    id.starts_with("PCI\\") || g.name.to_ascii_lowercase().contains("basic display")
}

fn has_high_refresh_display(s: &Snapshot) -> bool {
    s.displays.as_ref().is_some_and(|d| d.iter().any(|x| x.max_hz_any > 75))
}

// ---------------------------------------------------------------------------------------------
// 1. RAM below rated speed

fn ram_speed(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "ram_speed",
            title: "RAM running below its rated speed",
            impact: Impact::High,
            action: Action::Guide,
            explanation: "Gaming RAM is sold with a rated speed (XMP on Intel boards, EXPO or DOCP on AMD), but motherboards ship with it switched off, so the RAM runs at a slower safe default. That costs CPU-bound FPS and especially 1% lows, often by 10–20%.",
            fix_changes: "Turning on the memory profile in BIOS makes the RAM run at the speed it was sold at. Nothing else changes.",
            one_click_phase: None,
        },
        Guide {
            steps: steps(&[
                "Restart the PC and press the BIOS key as it starts (usually Delete or F2).",
                "Find the memory profile setting. It's called XMP on most Intel boards and EXPO, DOCP or A-XMP on AMD boards, often on the main or 'Ai Tweaker' / 'OC' page.",
                "Set it to Profile 1 (or Enabled), then save and exit (usually F10).",
                "The first boot after this can take a minute while the board trains the memory. If the PC won't start, it resets itself after a few tries, or you can clear CMOS.",
                "Back in Windows, press Recheck here to confirm the new speed.",
                "If the setting is already on but the speed is still low, your CPU's memory controller may cap it. Check your CPU's supported memory speed.",
            ]),
            settings_uri: None,
            links: vec![],
            choices: vec![],
        },
    );
    if snap.is_laptop == Some(true) {
        return set(f, Status::NotApplicable, "Laptops almost never let you change memory speed, so this check is skipped.");
    }
    let Some(mods) = snap.memory_modules.as_ref().filter(|m| !m.is_empty()) else {
        return couldnt_read(f, "the memory modules");
    };
    let configured = mods.iter().map(|m| m.configured_speed).filter(|&v| v > 0).min().unwrap_or(0);
    let rated = mods
        .iter()
        .map(|m| m.speed.max(speed_from_part_number(&m.part_number).unwrap_or(0)))
        .max()
        .unwrap_or(0);
    let mut f = f;
    f.details = mods
        .iter()
        .map(|m| {
            format!(
                "{} · {:.0} GB · {} · runs at {} MT/s (reports {} MT/s)",
                if m.device_locator.is_empty() { "Module" } else { &m.device_locator },
                gib(m.capacity_bytes),
                if m.part_number.trim().is_empty() { "unknown part" } else { m.part_number.trim() },
                m.configured_speed,
                m.speed
            )
        })
        .collect();
    if configured == 0 || rated == 0 {
        return couldnt_read(f, "the memory speed");
    }
    f.detected = Some(format!("{configured} MT/s"));
    f.expected = Some(format!("{rated} MT/s"));
    if (configured as f64) < rated as f64 * 0.97 {
        set(
            f,
            Status::Issue,
            format!("Your RAM runs at {configured} MT/s but is rated for {rated} MT/s."),
        )
    } else {
        set(f, Status::Ok, format!("Your RAM runs at its rated {configured} MT/s."))
    }
}

// ---------------------------------------------------------------------------------------------
// 2. Monitor below max refresh rate

fn refresh_rate(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "refresh_rate",
            title: "Monitor running below its maximum refresh rate",
            impact: Impact::High,
            action: Action::Fix,
            explanation: "A high-refresh monitor often comes out of the box at 60 Hz, and Windows doesn't always pick the highest rate. At 60 Hz you see at most 60 frames per second, however fast your PC is.",
            fix_changes: "Sets the display to the highest refresh rate it supports at its current resolution. Undo puts the previous rate back.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&[
                "Open Settings > System > Display > Advanced display (the button below opens it).",
                "Pick the monitor at the top if you have more than one.",
                "Under 'Choose a refresh rate', select the highest value.",
                "If the rate you expect isn't listed, check the cable: many monitors need DisplayPort or an HDMI 2.0+ cable for high refresh rates.",
            ]),
            settings_uri: Some(s("ms-settings:display-advanced")),
            links: vec![],
            choices: vec![],
        },
    );
    let Some(displays) = snap.displays.as_ref().filter(|d| !d.is_empty()) else {
        return couldnt_read(f, "the display modes");
    };
    let mut f = f;
    f.details = displays
        .iter()
        .map(|d| {
            format!(
                "{}{}: {}×{} at {} Hz (supports up to {} Hz at this resolution)",
                if d.monitor.is_empty() { &d.device_name } else { &d.monitor },
                if d.primary { " (main)" } else { "" },
                d.width,
                d.height,
                d.current_hz,
                d.max_hz_at_resolution
            )
        })
        .collect();
    let slow: Vec<&DisplayInfo> =
        displays.iter().filter(|d| d.max_hz_at_resolution >= d.current_hz + 5).collect();
    if let Some(worst) = slow.iter().max_by_key(|d| d.max_hz_at_resolution - d.current_hz) {
        f.detected = Some(format!("{} Hz", worst.current_hz));
        f.expected = Some(format!("{} Hz", worst.max_hz_at_resolution));
        let which = if displays.len() > 1 {
            if worst.primary { "Your main monitor" } else { "One of your monitors" }
        } else {
            "Your monitor"
        };
        set(
            f,
            Status::Issue,
            format!("{which} runs at {} Hz but supports {} Hz.", worst.current_hz, worst.max_hz_at_resolution),
        )
    } else {
        let hz = displays.iter().map(|d| d.current_hz).max().unwrap_or(0);
        f.detected = Some(format!("{hz} Hz"));
        set(f, Status::Ok, "Every monitor runs at its highest refresh rate.")
    }
}

// ---------------------------------------------------------------------------------------------
// 3. Single-channel RAM

fn ram_channels(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "ram_channels",
            title: "RAM running in single channel",
            impact: Impact::High,
            action: Action::Guide,
            explanation: "CPUs read memory over two channels at once. With one stick, or two sticks in the same channel, memory bandwidth is halved, which can cost 10–30% FPS in CPU-heavy games and hurts integrated graphics even more.",
            fix_changes: "Nothing in software. Two matching sticks in the right slots, or a second matching stick, restores dual channel.",
            one_click_phase: None,
        },
        Guide {
            steps: steps(&[
                "Shut down the PC and switch off the power supply.",
                "Check your motherboard manual for the dual-channel slots. With two sticks on a four-slot board it's usually the second and fourth slots from the CPU (A2 and B2).",
                "If you have one stick, add a second one with the same size and speed (ideally the same model).",
                "Move the sticks into the recommended slots, power on and press Recheck.",
                "On laptops, one slot may be soldered memory. Check the laptop's manual or a teardown before buying a stick.",
            ]),
            settings_uri: None,
            links: vec![],
            choices: vec![],
        },
    );
    let Some(mods) = snap.memory_modules.as_ref().filter(|m| !m.is_empty()) else {
        return couldnt_read(f, "the memory modules");
    };
    let mut f = f;
    f.details = mods
        .iter()
        .map(|m| {
            let ch = channel_letter(&m.device_locator, &m.bank_label)
                .map(|c| format!("channel {c}"))
                .unwrap_or_else(|| s("channel not reported"));
            format!("{} / {} · {:.0} GB · {ch}", m.device_locator, m.bank_label, gib(m.capacity_bytes))
        })
        .collect();
    f.detected = Some(format!("{} module{}", mods.len(), if mods.len() == 1 { "" } else { "s" }));
    if mods.len() == 1 {
        f.expected = Some(s("2 modules"));
        return set(f, Status::Issue, "Only one memory stick is installed, so the RAM runs in single channel.");
    }
    let letters: Vec<Option<char>> =
        mods.iter().map(|m| channel_letter(&m.device_locator, &m.bank_label)).collect();
    if letters.iter().all(|l| l.is_some()) {
        let first = letters[0];
        if letters.iter().all(|l| *l == first) {
            f.expected = Some(s("Sticks in different channels"));
            return set(
                f,
                Status::Issue,
                format!("All {} sticks are in channel {}, so the RAM runs in single channel.", mods.len(), first.unwrap()),
            );
        }
        return set(f, Status::Ok, format!("{} sticks spread across channels: dual channel.", mods.len()));
    }
    set(f, Status::Ok, format!("{} sticks installed. The board doesn't name channels, so dual channel is assumed.", mods.len()))
}

// ---------------------------------------------------------------------------------------------
// 4. Outdated GPU driver

const DRIVER_STALE_DAYS: i64 = 180;

fn gpu_driver(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "gpu_driver",
            title: "GPU driver out of date",
            impact: Impact::High,
            action: Action::Guide,
            explanation: "New GPU drivers include fixes and performance work for recent games. A driver more than six months old can miss them, and a PC with no GPU driver at all runs on Windows' basic display driver with no gaming acceleration.",
            fix_changes: "Installing the latest driver from your GPU maker. FrameWise creates a restore point first once fixes arrive; you can also make one yourself.",
            one_click_phase: None,
        },
        Guide {
            steps: steps(&[
                "Download the latest driver for your GPU from the maker's site (links below).",
                "Run the installer. Choose a clean or factory-reset install if games stutter or crash after past updates.",
                "Restart, then press Recheck.",
                "FrameWise checks the driver's age only. It doesn't contact NVIDIA, AMD or Intel, so it can't see the exact latest version.",
            ]),
            settings_uri: None,
            links: vec![
                link("NVIDIA drivers", "https://www.nvidia.com/en-us/drivers/"),
                link("AMD drivers", "https://www.amd.com/en/support/download/drivers.html"),
                link("Intel drivers", "https://www.intel.com/content/www/us/en/support/detect.html"),
            ],
            choices: vec![],
        },
    );
    let Some(gpus) = snap.gpus.as_ref() else {
        return couldnt_read(f, "the graphics adapters");
    };
    let real: Vec<&GpuInfo> = gpus.iter().filter(|g| is_real_gpu(g)).collect();
    let mut f = f;
    f.details = real
        .iter()
        .map(|g| {
            format!(
                "{} · driver {} · dated {}",
                g.name,
                g.driver_version,
                g.driver_date.as_deref().map(pretty_date).unwrap_or_else(|| s("unknown"))
            )
        })
        .collect();
    if real.is_empty() {
        return couldnt_read(f, "a physical graphics adapter");
    }
    if let Some(basic) = real.iter().find(|g| g.name.to_ascii_lowercase().contains("basic display")) {
        f.detected = Some(basic.name.clone());
        f.expected = Some(s("Your GPU maker's driver"));
        return set(f, Status::Issue, "No GPU driver is installed. Windows is using its basic display driver.");
    }
    let today = (snap.collected_at / 86_400) as i64;
    let oldest = real
        .iter()
        .filter_map(|g| g.driver_date.as_deref().and_then(yyyymmdd_to_days).map(|d| (g, today - d)))
        .max_by_key(|(_, age)| *age);
    let Some((gpu, age)) = oldest else {
        return couldnt_read(f, "the driver date");
    };
    f.detected = Some(format!(
        "{} ({} days old)",
        gpu.driver_date.as_deref().map(pretty_date).unwrap_or_default(),
        age.max(0)
    ));
    f.expected = Some(format!("Under {DRIVER_STALE_DAYS} days old"));
    if age > DRIVER_STALE_DAYS {
        set(f, Status::Issue, format!("Your {} driver is {} months old.", gpu.name, age / 30))
    } else {
        set(f, Status::Ok, format!("Your {} driver is recent ({} days old).", gpu.name, age.max(0)))
    }
}

// ---------------------------------------------------------------------------------------------
// 5. Thermal throttling

fn thermal_throttling(snap: &Snapshot) -> Finding {
    let laptop = snap.is_laptop == Some(true);
    let f = finding(
        Meta {
            id: "thermal_throttling",
            title: "CPU or GPU thermal throttling",
            impact: Impact::High,
            action: Action::Guide,
            explanation: "When a CPU or GPU gets too hot it lowers its own clock speed to protect itself. In games that shows up as FPS that starts fine and then drops, or sudden stutters after a few minutes of play.",
            fix_changes: "Nothing in software. Cleaning dust, improving airflow or adjusting fan curves lets the chips hold their full speed.",
            one_click_phase: None,
        },
        Guide {
            steps: if laptop {
                steps(&[
                    "Play on a hard, flat surface. Beds and laps block the air intakes underneath.",
                    "Blow dust out of the vents with compressed air, in short bursts, with the laptop off.",
                    "Use the maker's performance or fan mode (for example in Armoury Crate, Lenovo Vantage or Omen Gaming Hub) while gaming.",
                    "A cooling pad helps on laptops that take air in from the bottom.",
                ])
            } else {
                steps(&[
                    "Shut down and clean dust from the CPU cooler, GPU fans, case filters and power supply with compressed air.",
                    "Check that case fans move air front-to-back or bottom-to-top, and that every fan spins.",
                    "In BIOS, set a fan curve that ramps up above about 70 °C.",
                    "If the CPU is more than four or five years old, its thermal paste may have dried out.",
                ])
            },
            settings_uri: None,
            links: vec![],
            choices: vec![],
        },
    );
    set(
        f,
        Status::Unknown,
        "Throttling only shows under load. Start the monitor (Monitor & benchmark) while you play: it watches temperatures and clocks and warns if the CPU or GPU throttles.",
    )
}

// ---------------------------------------------------------------------------------------------
// 6. Game installed on HDD

fn game_on_hdd(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "game_on_hdd",
            title: "Games installed on a hard drive",
            impact: Impact::Medium,
            action: Action::Guide,
            explanation: "Modern games stream textures and levels while you play. From a spinning hard drive that causes long loads, pop-in and hitching; an SSD is many times faster for this.",
            fix_changes: "Moving the game to an SSD with the launcher's own move tool. Saves and settings come with it.",
            one_click_phase: None,
        },
        Guide {
            steps: steps(&[
                "Steam: Settings > Storage, add a library folder on your SSD, then select the game and choose Move.",
                "Epic Games: there's no move button, so uninstall the game and reinstall it to a folder on the SSD.",
                "Xbox app: open the game's page, choose Manage > Files > Move.",
                "Battle.net: close Battle.net, move the game's folder to the SSD, then use 'Locate the game' on the game's page.",
                "Prioritise the games you play most, especially open-world games.",
            ]),
            settings_uri: None,
            links: vec![],
            choices: vec![],
        },
    );
    let (Some(games), Some(drives)) = (snap.game_installs.as_ref(), snap.drives.as_ref()) else {
        return couldnt_read(f, "the game libraries");
    };
    if games.is_empty() {
        return set(f, Status::NotApplicable, "No Steam or Epic games found. Other launchers come with game profiles in phase 4.");
    }
    let media_for = |path: &str| -> MediaType {
        let p = path.to_ascii_lowercase();
        drives
            .iter()
            .filter(|d| p.starts_with(&d.mount.to_ascii_lowercase()))
            .max_by_key(|d| d.mount.len())
            .map(|d| d.media)
            .unwrap_or(MediaType::Unknown)
    };
    let on_hdd: Vec<&GameInstall> = games.iter().filter(|g| media_for(&g.path) == MediaType::Hdd).collect();
    let mut f = f;
    f.detected = Some(format!("{} of {} games on a hard drive", on_hdd.len(), games.len()));
    f.expected = Some(s("All games on an SSD"));
    f.details = on_hdd.iter().map(|g| format!("{} ({}) · {}", g.name, g.launcher, g.path)).collect();
    if on_hdd.is_empty() {
        set(f, Status::Ok, format!("All {} detected games are on an SSD.", games.len()))
    } else {
        set(
            f,
            Status::Issue,
            format!("{} of your {} games {} on a hard drive.", on_hdd.len(), games.len(), if on_hdd.len() == 1 { "is" } else { "are" }),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// 7. Overlays and launchers running

/// Known overlays, recorders and launchers that run in the background (lower-case exe names).
pub const KNOWN_OVERLAYS: &[(&str, &str)] = &[
    ("discord.exe", "Discord (overlay)"),
    ("nvidia overlay.exe", "NVIDIA overlay"),
    ("nvidia share.exe", "NVIDIA ShadowPlay"),
    ("epicgameslauncher.exe", "Epic Games Launcher"),
    ("battle.net.exe", "Battle.net"),
    ("eadesktop.exe", "EA app"),
    ("origin.exe", "Origin"),
    ("upc.exe", "Ubisoft Connect"),
    ("ubisoftconnect.exe", "Ubisoft Connect"),
    ("galaxyclient.exe", "GOG Galaxy"),
    ("riotclientservices.exe", "Riot Client"),
    ("overwolf.exe", "Overwolf"),
    ("medal.exe", "Medal"),
    ("obs64.exe", "OBS Studio"),
    ("msiafterburner.exe", "MSI Afterburner"),
    ("rtss.exe", "RivaTuner Statistics Server"),
    ("razercortex.exe", "Razer Cortex"),
    ("icue.exe", "Corsair iCUE"),
    ("armourycrate.userssessionhelper.exe", "Armoury Crate"),
    ("wallpaper64.exe", "Wallpaper Engine"),
    ("wallpaper32.exe", "Wallpaper Engine"),
    ("signalrgb.exe", "SignalRGB"),
    ("lghub.exe", "Logitech G HUB"),
];

const OVERLAY_THRESHOLD: usize = 3;

fn overlays(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "overlays",
            title: "Overlays and launchers running in the background",
            impact: Impact::Medium,
            action: Action::Fix,
            explanation: "Overlays hook into the game's rendering, and launchers, recorders and RGB apps wake up regularly in the background. A few are fine; several together add CPU work and can cause stutter.",
            fix_changes: "A game profile closes or suspends the apps you pick while a game runs and brings them back when it exits. Nothing is uninstalled.",
            one_click_phase: Some(4),
        },
        Guide {
            steps: steps(&[
                "Quit launchers you don't need open (right-click their tray icon > Exit).",
                "Turn off overlays you don't use: Discord (Settings > Game Overlay), NVIDIA app or GeForce Experience (in-game overlay), Steam (Settings > In Game).",
                "Stop apps launching with Windows in Settings > Apps > Startup (see the Startup check).",
                "From phase 4, game profiles do this automatically for each game and undo it on exit.",
            ]),
            settings_uri: None,
            links: vec![],
            choices: vec![],
        },
    );
    let Some(procs) = snap.processes.as_ref() else {
        return couldnt_read(f, "the running apps");
    };
    let mut found: Vec<&str> = KNOWN_OVERLAYS
        .iter()
        .filter(|(exe, _)| procs.iter().any(|p| p == exe))
        .map(|(_, label)| *label)
        .collect();
    found.dedup();
    let mut f = f;
    f.detected = Some(format!("{} running", found.len()));
    f.expected = Some(format!("Fewer than {OVERLAY_THRESHOLD} while gaming"));
    f.details = found.iter().map(|x| s(x)).collect();
    if found.len() >= OVERLAY_THRESHOLD {
        set(f, Status::Issue, format!("{} overlays or launchers are running right now.", found.len()))
    } else if found.is_empty() {
        set(f, Status::Ok, "No known overlays or launchers are running.")
    } else {
        set(f, Status::Ok, format!("{} running, which is a light load.", found.len()))
    }
}

// ---------------------------------------------------------------------------------------------
// 8. Startup bloat

const STARTUP_THRESHOLD: usize = 8;

fn startup_bloat(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "startup_bloat",
            title: "Too many apps starting with Windows",
            impact: Impact::Medium,
            action: Action::Fix,
            explanation: "Every app that starts with Windows uses memory and wakes the CPU in the background, all the time, including while you game. Most updaters and helpers don't need to run until you open the app.",
            fix_changes: "Disables the startup entries you select, the same way Task Manager does. The apps still work when you open them, and Undo re-enables them.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&[
                "Open Settings > Apps > Startup (the button below opens it), or Task Manager > Startup apps.",
                "Turn off apps you don't need the moment Windows starts: updaters, launchers, cloud tools you rarely use, RGB tools you've already configured.",
                "Keep security software, audio and touchpad drivers, and anything you rely on in the tray.",
                "Restart, then press Recheck.",
            ]),
            settings_uri: Some(s("ms-settings:startupapps")),
            links: vec![],
            choices: vec![],
        },
    );
    let Some(entries) = snap.startup_entries.as_ref() else {
        return couldnt_read(f, "the startup entries");
    };
    // FrameWise's own profile watcher only runs because the user turned profiles on, and stays
    // within its own small budget, so it isn't counted against them.
    let enabled: Vec<&StartupEntry> = entries.iter().filter(|e| e.enabled && e.name != "FrameWise watcher").collect();
    let mut f = f;
    f.detected = Some(format!("{} enabled", enabled.len()));
    f.expected = Some(format!("{STARTUP_THRESHOLD} or fewer"));
    f.details = enabled.iter().map(|e| format!("{} · {}", e.name, e.location)).collect();
    if enabled.len() > STARTUP_THRESHOLD {
        set(f, Status::Issue, format!("{} apps start with Windows.", enabled.len()))
    } else {
        set(f, Status::Ok, format!("{} apps start with Windows, which is reasonable.", enabled.len()))
    }
}

// ---------------------------------------------------------------------------------------------
// 9. Power plan

pub use crate::tweak::{
    scheme_name, MODE_BEST_EFFICIENCY as OVERLAY_BEST_EFFICIENCY, SCHEME_BALANCED, SCHEME_HIGH, SCHEME_SAVER, SCHEME_ULTIMATE,
};

fn power_plan(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "power_plan",
            title: "Power plan limits performance",
            impact: Impact::Medium,
            action: Action::Fix,
            explanation: "The Power saver plan, and Windows 11's 'Best power efficiency' mode, hold the CPU at lower clock speeds to save energy. On a desktop that costs FPS for very little saving. Balanced is the right default; High performance helps a little in some games.",
            fix_changes: "Switches to the Balanced plan (or a game profile switches to High performance only while a game runs). Undo restores the previous plan.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&[
                "Open Settings > System > Power (the button below opens it).",
                "Set Power mode to Balanced or Best performance.",
                "On Windows 10, open Control Panel > Power Options and choose Balanced or High performance.",
            ]),
            settings_uri: Some(s("ms-settings:powersleep")),
            links: vec![],
            choices: vec![],
        },
    );
    let Some(active) = snap.power.active_scheme.as_deref() else {
        return couldnt_read(f, "the active power plan");
    };
    let name = scheme_name(active);
    let mut f = f;
    f.detected = Some(s(name));
    f.expected = Some(s("Balanced or High performance"));
    if active == SCHEME_SAVER {
        return set(f, Status::Issue, "The Power saver plan is active, which holds back the CPU.");
    }
    if snap.power.ac_overlay.as_deref() == Some(OVERLAY_BEST_EFFICIENCY) {
        f.detected = Some(format!("{name} · Best power efficiency mode"));
        return set(f, Status::Issue, "Power mode is set to 'Best power efficiency', which holds back the CPU.");
    }
    set(f, Status::Ok, format!("{name} plan is active."))
}

// ---------------------------------------------------------------------------------------------
// 10. Game Mode

fn game_mode(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "game_mode",
            title: "Game Mode is off",
            impact: Impact::Low,
            action: Action::Fix,
            explanation: "Game Mode tells Windows to hold back Windows Update installs and driver updates and to give the game priority for CPU time while it runs. It's on by default and rarely hurts.",
            fix_changes: "Turns Game Mode on (one Windows setting). Undo turns it back off.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&["Open Settings > Gaming > Game Mode (the button below opens it).", "Turn Game Mode on."]),
            settings_uri: Some(s("ms-settings:gaming-gamemode")),
            links: vec![],
            choices: vec![],
        },
    );
    let mut f = f;
    f.expected = Some(s("On"));
    match snap.game_mode_auto {
        Some(0) => {
            f.detected = Some(s("Off"));
            set(f, Status::Issue, "Game Mode is turned off.")
        }
        _ => {
            f.detected = Some(s("On"));
            set(f, Status::Ok, "Game Mode is on.")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 11. Hardware-accelerated GPU scheduling

fn hags(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "hags",
            title: "Hardware-accelerated GPU scheduling is off",
            impact: Impact::Low,
            action: Action::Fix,
            explanation: "This lets the GPU manage its own work queue instead of the CPU, which slightly lowers latency and CPU load. NVIDIA DLSS Frame Generation and AMD's equivalent need it.",
            fix_changes: "Turns the setting on. It takes effect after a restart. Undo turns it back off.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&[
                "Open Settings > System > Display > Graphics (the button below opens it).",
                "Select 'Change default graphics settings' (on Windows 11, it may be under 'Advanced graphics settings').",
                "Turn on 'Hardware-accelerated GPU scheduling' and restart.",
                "If the switch isn't there, your GPU or driver doesn't support it. Updating the GPU driver may add it.",
            ]),
            settings_uri: Some(s("ms-settings:display-advancedgraphics")),
            links: vec![],
            choices: vec![],
        },
    );
    let mut f = f;
    f.expected = Some(s("On"));
    if let Some(caps) = snap.hags_caps {
        return match (caps.supported, caps.enabled) {
            (false, _) => {
                f.detected = Some(s("Not supported"));
                set(f, Status::NotApplicable, "Your GPU or driver doesn't support hardware-accelerated GPU scheduling.")
            }
            (true, true) => {
                f.detected = Some(s("On"));
                set(f, Status::Ok, "Hardware-accelerated GPU scheduling is on.")
            }
            (true, false) => {
                f.detected = Some(s("Off"));
                set(f, Status::Issue, "Your GPU supports hardware-accelerated GPU scheduling, but it's off.")
            }
        };
    }
    match snap.hags_mode {
        Some(2) => {
            f.detected = Some(s("On"));
            set(f, Status::Ok, "Hardware-accelerated GPU scheduling is on.")
        }
        Some(_) => {
            f.detected = Some(s("Off"));
            set(f, Status::Issue, "Hardware-accelerated GPU scheduling is turned off.")
        }
        None => {
            f.detected = Some(s("Not set"));
            set(f, Status::Unknown, "Windows hasn't recorded this setting. Your GPU may not support it; check the Graphics settings page.")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 12. Optimisations for windowed games

fn windowed_optimizations(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "windowed_optimizations",
            title: "Optimisations for windowed games are off",
            impact: Impact::Low,
            action: Action::Fix,
            explanation: "Many games run in borderless windowed mode. On Windows 11 this setting lets those games present frames like fullscreen games do, lowering latency and enabling Auto HDR and variable refresh in older DirectX games.",
            fix_changes: "Turns the setting on in Windows' graphics preferences. Undo turns it back off.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&[
                "Open Settings > System > Display > Graphics (the button below opens it).",
                "Select 'Change default graphics settings' or 'Advanced graphics settings'.",
                "Turn on 'Optimizations for windowed games'.",
            ]),
            settings_uri: Some(s("ms-settings:display-advancedgraphics")),
            links: vec![],
            choices: vec![],
        },
    );
    if !snap.os.is_windows_11() {
        return set(f, Status::NotApplicable, "This setting only exists on Windows 11.");
    }
    let setting = snap.dx_user_global_settings.as_deref().unwrap_or("");
    let explicit = if setting.contains("SwapEffectUpgradeEnable=1") {
        Some(true)
    } else if setting.contains("SwapEffectUpgradeEnable=0") {
        Some(false)
    } else {
        None
    };
    // Windows 11 24H2 (build 26100) and later turn this on by default.
    let on = explicit.unwrap_or(snap.os.build >= 26100);
    let mut f = f;
    f.expected = Some(s("On"));
    f.detected = Some(s(match (on, explicit.is_some()) {
        (true, true) => "On",
        (true, false) => "On (Windows default)",
        (false, true) => "Off",
        (false, false) => "Off (Windows default)",
    }));
    if on {
        set(f, Status::Ok, "Optimisations for windowed games are on.")
    } else {
        set(f, Status::Issue, "Optimisations for windowed games are off.")
    }
}

// ---------------------------------------------------------------------------------------------
// 13. Memory Integrity

fn memory_integrity(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "memory_integrity",
            title: "Memory Integrity is on",
            impact: Impact::LowMedium,
            action: Action::Choose,
            explanation: "Memory Integrity (also called HVCI, part of Core isolation) uses virtualisation to stop malicious drivers from tampering with Windows. It protects you, and on some CPUs, mostly older ones, it costs a few percent of FPS. This is your call; FrameWise never changes it automatically and doesn't count it against your score.",
            fix_changes: "If you choose to turn it off: one Windows Security setting, which needs a restart. You can turn it back on at any time.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&[
                "Open Windows Security > Device security > Core isolation details (the button below opens it).",
                "Switch Memory integrity on or off, then restart.",
                "If you're unsure, record benchmarks with it on and off (Monitor & benchmark) and keep it on unless the difference is clear.",
            ]),
            settings_uri: Some(s("windowsdefender://coreisolation")),
            links: vec![],
            choices: vec![
                Choice {
                    label: s("Keep it on (recommended for most people)"),
                    gains: s("Strong protection against malicious and vulnerable drivers, which some cheats and malware use."),
                    costs: s("Typically 0–5% FPS, more on CPUs older than about 2018 (Intel 7th gen, AMD Zen 1)."),
                },
                Choice {
                    label: s("Turn it off"),
                    gains: s("Recovers that small FPS cost, mostly on older CPUs."),
                    costs: s("Removes a layer of protection. Some anti-cheat systems and work or school policies require it on."),
                },
            ],
        },
    );
    let mut f = f;
    // The registry switch is what Windows will use after the next restart; `hvci_running` is what
    // it's doing now. They differ until a restart, or when the PC can't run Memory Integrity.
    match (snap.hvci_enabled, snap.hvci_running) {
        (Some(1), Some(false)) => {
            f.detected = Some(s("Switched on, not running yet"));
            f.title = s("Memory Integrity is switched on but not running");
            set(
                f,
                Status::Issue,
                "Memory Integrity is switched on but isn't running yet, so it costs nothing right now. It starts after a restart; if it still isn't running after one, this PC can't run it.",
            )
        }
        (Some(1), _) => {
            f.detected = Some(s("On"));
            set(f, Status::Issue, "Memory Integrity is on. That's the secure choice; see the trade-off below.")
        }
        (_, Some(true)) => {
            f.detected = Some(s("Switched off, still running until restart"));
            f.title = s("Memory Integrity is off after a restart");
            set(f, Status::Ok, "Memory Integrity is switched off, but keeps running (and protecting you) until you restart.")
        }
        (Some(_), _) => {
            f.detected = Some(s("Off"));
            f.title = s("Memory Integrity is off");
            set(f, Status::Ok, "Memory Integrity is off, so there's no FPS cost from it. Turning it on adds protection.")
        }
        (None, _) => {
            f.detected = Some(s("Off (not configured)"));
            f.title = s("Memory Integrity is off");
            set(f, Status::Ok, "Memory Integrity isn't configured, so there's no FPS cost from it.")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 14. Variable refresh rate

fn variable_refresh(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "variable_refresh",
            title: "Variable refresh (G-Sync / FreeSync) may be off",
            impact: Impact::Medium,
            action: Action::Guide,
            explanation: "Variable refresh makes the monitor wait for each frame instead of the other way round. It removes tearing and the stutter of normal V-Sync when FPS drops below the refresh rate, which makes uneven frame rates feel much smoother.",
            fix_changes: "Turning it on in the monitor's menu and your GPU control panel. Nothing in Windows changes.",
            one_click_phase: None,
        },
        Guide {
            steps: steps(&[
                "In the monitor's own on-screen menu, turn on FreeSync, Adaptive-Sync or VRR (the name varies).",
                "NVIDIA: open the NVIDIA Control Panel > Display > Set up G-SYNC, tick 'Enable G-SYNC, G-SYNC Compatible' and apply.",
                "AMD: open AMD Software > Gaming > Display and turn on AMD FreeSync.",
                "Intel Arc: open Intel Graphics Software > Display and turn on Adaptive Sync.",
                "Use DisplayPort where possible; many monitors only support variable refresh over it.",
            ]),
            settings_uri: None,
            links: vec![],
            choices: vec![],
        },
    );
    if snap.displays.is_some() && !has_high_refresh_display(snap) {
        return set(f, Status::NotApplicable, "None of your monitors go above 75 Hz, so variable refresh is unlikely to be available.");
    }
    set(
        f,
        Status::Unknown,
        "FrameWise can't read GPU control panel settings yet. If you have a high-refresh monitor, check that it's on.",
    )
}

// ---------------------------------------------------------------------------------------------
// 15. System drive nearly full

fn system_drive_space(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "system_drive_space",
            title: "System drive nearly full",
            impact: Impact::Low,
            action: Action::Fix,
            explanation: "Windows needs free space on its drive for the page file, updates and shader caches. Under about 10% free, updates fail, games can't cache shaders and SSDs slow down.",
            fix_changes: "Cleanup removes temp files, crash dumps, old driver packages and update leftovers, and shows their size first. Your own files go to the Recycle Bin.",
            one_click_phase: Some(2),
        },
        Guide {
            steps: steps(&[
                "Open Settings > System > Storage (the button below opens it) and select 'Temporary files' to remove what Windows lists.",
                "Turn on Storage Sense to clear temp files automatically.",
                "Uninstall games you no longer play, or move them to another drive with the launcher's move tool.",
                "Empty the Recycle Bin.",
            ]),
            settings_uri: Some(s("ms-settings:storagesense")),
            links: vec![],
            choices: vec![],
        },
    );
    let (Some(drives), Some(sys)) = (snap.drives.as_ref(), snap.system_drive.as_deref()) else {
        return couldnt_read(f, "the system drive");
    };
    let sys_l = sys.to_ascii_lowercase();
    let Some(d) = drives.iter().find(|d| d.mount.to_ascii_lowercase().starts_with(&sys_l)) else {
        return couldnt_read(f, "the system drive");
    };
    if d.total_bytes == 0 {
        return couldnt_read(f, "the system drive size");
    }
    let pct = d.free_bytes as f64 / d.total_bytes as f64 * 100.0;
    let mut f = f;
    f.detected = Some(format!("{:.0} GB free of {:.0} GB ({pct:.0}%)", gib(d.free_bytes), gib(d.total_bytes)));
    f.expected = Some(s("At least 10% free"));
    if pct < 10.0 {
        set(f, Status::Issue, format!("Only {pct:.0}% of drive {} is free.", d.mount.trim_end_matches('\\')))
    } else {
        set(f, Status::Ok, format!("{pct:.0}% of drive {} is free.", d.mount.trim_end_matches('\\')))
    }
}

// ---------------------------------------------------------------------------------------------
// 16. High ping or jitter

const PING_LIMIT_MS: f64 = 60.0;
const JITTER_LIMIT_MS: f64 = 15.0;

fn network_latency(snap: &Snapshot) -> Finding {
    let f = finding(
        Meta {
            id: "network_latency",
            title: "High ping or unstable connection",
            impact: Impact::Medium,
            action: Action::Guide,
            explanation: "In online games, high ping delays everything you do, and jitter (ping that jumps around) causes rubber-banding. Wi-Fi and a busy home network are the usual causes, not the PC.",
            fix_changes: "Nothing on the PC. A wired connection or router settings fix most cases.",
            one_click_phase: None,
        },
        Guide {
            steps: steps(&[
                "Use an ethernet cable if you can. It's the single biggest improvement for jitter.",
                "On Wi-Fi, use the 5 GHz or 6 GHz band and sit closer to the router.",
                "If ping spikes when someone else streams or downloads, turn on your router's QoS or SQM setting (sometimes called 'bufferbloat' control).",
                "Pause cloud backups, torrent clients and game downloads while playing.",
                "This test measures connection time to a public DNS server with no data sent. A full under-load (bufferbloat) test is planned.",
            ]),
            settings_uri: Some(s("ms-settings:network-status")),
            links: vec![],
            choices: vec![],
        },
    );
    let Some(probe) = snap.network.as_ref() else {
        return couldnt_read(f, "the network");
    };
    if probe.samples_ms.is_empty() {
        return set(f, Status::Unknown, "Couldn't reach the internet to measure ping.");
    }
    let med = median(&probe.samples_ms).unwrap_or(0.0);
    let jit = jitter(&probe.samples_ms).unwrap_or(0.0);
    let mut f = f;
    f.detected = Some(format!("{med:.0} ms ping, {jit:.0} ms jitter"));
    f.expected = Some(format!("Under {PING_LIMIT_MS:.0} ms ping, under {JITTER_LIMIT_MS:.0} ms jitter"));
    f.details = vec![format!(
        "{} samples to {} · {} failed",
        probe.samples_ms.len(),
        probe.target,
        probe.failures
    )];
    if med > PING_LIMIT_MS || jit > JITTER_LIMIT_MS || probe.failures > 1 {
        let what = if jit > JITTER_LIMIT_MS || probe.failures > 1 { "unstable" } else { "slow" };
        set(f, Status::Issue, format!("Your connection is {what}: {med:.0} ms ping with {jit:.0} ms jitter."))
    } else {
        set(f, Status::Ok, format!("Connection is healthy: {med:.0} ms ping, {jit:.0} ms jitter."))
    }
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn dimm(loc: &str, speed: u32, configured: u32, part: &str) -> MemoryModule {
        MemoryModule {
            capacity_bytes: 16 * 1024 * 1024 * 1024,
            speed,
            configured_speed: configured,
            manufacturer: s("Corsair"),
            part_number: s(part),
            bank_label: s("BANK 0"),
            device_locator: s(loc),
            smbios_memory_type: 26,
        }
    }

    fn desktop() -> Snapshot {
        Snapshot {
            collected_at: days_from_civil(2026, 11, 7) as u64 * 86_400,
            os: OsInfo { build: 26100, display_version: Some(s("24H2")) },
            is_laptop: Some(false),
            ..Default::default()
        }
    }

    fn status(snap: &Snapshot, id: &str) -> Status {
        evaluate_one(snap, id).unwrap().status
    }

    /// A recorded snapshot of a typical mid-range gaming desktop with common problems.
    #[test]
    fn recorded_gaming_desktop() {
        let snap: Snapshot = serde_json::from_str(include_str!("../fixtures/gaming-desktop.json")).unwrap();
        let by_id = |id: &str| evaluate_one(&snap, id).unwrap();
        assert_eq!(by_id("ram_speed").status, Status::Issue);
        assert_eq!(by_id("ram_speed").expected.as_deref(), Some("3600 MT/s"));
        assert_eq!(by_id("ram_channels").status, Status::Ok);
        assert_eq!(by_id("refresh_rate").status, Status::Issue);
        assert_eq!(by_id("gpu_driver").status, Status::Ok);
        assert_eq!(by_id("game_on_hdd").details.len(), 2);
        assert_eq!(by_id("overlays").status, Status::Issue);
        assert_eq!(by_id("startup_bloat").status, Status::Issue);
        assert_eq!(by_id("power_plan").status, Status::Ok);
        assert_eq!(by_id("hags").status, Status::Issue);
        assert_eq!(by_id("memory_integrity").action, Action::Choose);
        assert_eq!(by_id("network_latency").status, Status::Ok);
        // 2 High (15) + 3 Medium (8) + 1 Low (3); Memory Integrity is a choice and doesn't count.
        assert_eq!(crate::health_score(&evaluate_all(&snap)), 100 - 30 - 24 - 3);
    }

    #[test]
    fn every_check_evaluates_on_an_empty_snapshot() {
        let findings = evaluate_all(&Snapshot::default());
        assert_eq!(findings.len(), 16);
        let ids: Vec<&str> = findings.iter().map(|f| f.check_id.as_str()).collect();
        assert_eq!(ids, CHECK_IDS);
        for f in &findings {
            assert!(!f.summary.is_empty(), "{} has no summary", f.check_id);
            assert!(!f.guide.steps.is_empty(), "{} has no guide steps", f.check_id);
            // Nothing read means nothing claimed.
            assert_ne!(f.status, Status::Issue, "{} raised an issue with no data", f.check_id);
        }
    }

    #[test]
    fn xmp_off_is_detected_from_part_number() {
        let mut snap = desktop();
        snap.memory_modules = Some(vec![
            dimm("DIMM_A2", 2133, 2133, "CMK16GX4M2B3600C18"),
            dimm("DIMM_B2", 2133, 2133, "CMK16GX4M2B3600C18"),
        ]);
        let f = evaluate_one(&snap, "ram_speed").unwrap();
        assert_eq!(f.status, Status::Issue);
        assert_eq!(f.expected.as_deref(), Some("3600 MT/s"));
        assert_eq!(status(&snap, "ram_channels"), Status::Ok);
    }

    #[test]
    fn xmp_on_is_ok() {
        let mut snap = desktop();
        snap.memory_modules = Some(vec![
            dimm("DIMM_A2", 3600, 3600, "CMK16GX4M2B3600C18"),
            dimm("DIMM_B2", 3600, 3600, "CMK16GX4M2B3600C18"),
        ]);
        assert_eq!(status(&snap, "ram_speed"), Status::Ok);
    }

    #[test]
    fn ram_speed_skipped_on_laptops() {
        let mut snap = desktop();
        snap.is_laptop = Some(true);
        snap.memory_modules = Some(vec![dimm("DIMM_A1", 3200, 2666, "")]);
        assert_eq!(status(&snap, "ram_speed"), Status::NotApplicable);
    }

    #[test]
    fn single_channel_cases() {
        let mut snap = desktop();
        snap.memory_modules = Some(vec![dimm("DIMM_A2", 3200, 3200, "")]);
        assert_eq!(status(&snap, "ram_channels"), Status::Issue);
        snap.memory_modules = Some(vec![dimm("ChannelA-DIMM0", 3200, 3200, ""), dimm("ChannelA-DIMM1", 3200, 3200, "")]);
        assert_eq!(status(&snap, "ram_channels"), Status::Issue);
        snap.memory_modules = Some(vec![dimm("DIMM 0", 3200, 3200, ""), dimm("DIMM 1", 3200, 3200, "")]);
        assert_eq!(status(&snap, "ram_channels"), Status::Ok);
    }

    #[test]
    fn refresh_rate_stuck_at_60() {
        let mut snap = desktop();
        snap.displays = Some(vec![DisplayInfo {
            device_name: s("\\\\.\\DISPLAY1"),
            monitor: s("Generic PnP Monitor"),
            primary: true,
            width: 2560,
            height: 1440,
            current_hz: 60,
            max_hz_at_resolution: 165,
            max_hz_any: 165,
            ..Default::default()
        }]);
        let f = evaluate_one(&snap, "refresh_rate").unwrap();
        assert_eq!(f.status, Status::Issue);
        assert_eq!(f.expected.as_deref(), Some("165 Hz"));
        assert_eq!(status(&snap, "variable_refresh"), Status::Unknown);
        // 59 vs 60 Hz is the same mode family; don't flag it.
        snap.displays.as_mut().unwrap()[0].current_hz = 59;
        snap.displays.as_mut().unwrap()[0].max_hz_at_resolution = 60;
        snap.displays.as_mut().unwrap()[0].max_hz_any = 60;
        assert_eq!(status(&snap, "refresh_rate"), Status::Ok);
        assert_eq!(status(&snap, "variable_refresh"), Status::NotApplicable);
    }

    #[test]
    fn driver_age_and_basic_display() {
        let mut snap = desktop();
        let gpu = |name: &str, date: &str| GpuInfo {
            name: s(name),
            vendor: s("NVIDIA"),
            driver_version: s("32.0.15.6094"),
            driver_date: Some(s(date)),
            pnp_device_id: s("PCI\\VEN_10DE&DEV_2684"),
        };
        snap.gpus = Some(vec![gpu("NVIDIA GeForce RTX 4090", "20260901")]);
        assert_eq!(status(&snap, "gpu_driver"), Status::Ok);
        snap.gpus = Some(vec![gpu("NVIDIA GeForce RTX 4090", "20250101")]);
        assert_eq!(status(&snap, "gpu_driver"), Status::Issue);
        snap.gpus = Some(vec![gpu("Microsoft Basic Display Adapter", "20060621")]);
        let f = evaluate_one(&snap, "gpu_driver").unwrap();
        assert_eq!(f.status, Status::Issue);
        assert!(f.summary.contains("basic display"));
        // Virtual adapters (remote desktop, streaming) are ignored.
        snap.gpus = Some(vec![GpuInfo { pnp_device_id: s("ROOT\\DISPLAY\\0000"), ..gpu("Parsec Virtual Display", "20200101") }]);
        assert_eq!(status(&snap, "gpu_driver"), Status::Unknown);
    }

    #[test]
    fn games_on_hdd() {
        let mut snap = desktop();
        snap.drives = Some(vec![
            DriveInfo { mount: s("C:\\"), media: MediaType::Ssd, total_bytes: 1, free_bytes: 1 },
            DriveInfo { mount: s("D:\\"), media: MediaType::Hdd, total_bytes: 1, free_bytes: 1 },
        ]);
        snap.game_installs = Some(vec![
            GameInstall { name: s("Cyberpunk 2077"), launcher: s("Steam"), path: s("D:\\SteamLibrary\\steamapps\\common\\Cyberpunk 2077") },
            GameInstall { name: s("Fortnite"), launcher: s("Epic"), path: s("C:\\Program Files\\Epic Games\\Fortnite") },
        ]);
        let f = evaluate_one(&snap, "game_on_hdd").unwrap();
        assert_eq!(f.status, Status::Issue);
        assert_eq!(f.details.len(), 1);
        snap.game_installs = Some(vec![]);
        assert_eq!(status(&snap, "game_on_hdd"), Status::NotApplicable);
    }

    #[test]
    fn overlays_threshold() {
        let mut snap = desktop();
        snap.processes = Some(vec![s("discord.exe"), s("explorer.exe")]);
        assert_eq!(status(&snap, "overlays"), Status::Ok);
        snap.processes = Some(vec![s("discord.exe"), s("epicgameslauncher.exe"), s("overwolf.exe"), s("icue.exe")]);
        assert_eq!(status(&snap, "overlays"), Status::Issue);
    }

    #[test]
    fn startup_counts_only_enabled() {
        let mut snap = desktop();
        let entry = |i: usize, enabled: bool| StartupEntry { name: format!("App{i}"), location: s("HKCU Run"), enabled, source: None, value_name: format!("App{i}"), command: String::new() };
        snap.startup_entries = Some((0..12).map(|i| entry(i, i < 5)).collect());
        assert_eq!(status(&snap, "startup_bloat"), Status::Ok);
        snap.startup_entries = Some((0..12).map(|i| entry(i, true)).collect());
        assert_eq!(status(&snap, "startup_bloat"), Status::Issue);
    }

    #[test]
    fn power_plans() {
        let mut snap = desktop();
        snap.power.active_scheme = Some(s(SCHEME_BALANCED));
        assert_eq!(status(&snap, "power_plan"), Status::Ok);
        snap.power.active_scheme = Some(s(SCHEME_SAVER));
        assert_eq!(status(&snap, "power_plan"), Status::Issue);
        snap.power.active_scheme = Some(s(SCHEME_BALANCED));
        snap.power.ac_overlay = Some(s(OVERLAY_BEST_EFFICIENCY));
        assert_eq!(status(&snap, "power_plan"), Status::Issue);
    }

    #[test]
    fn registry_toggles() {
        let mut snap = desktop();
        assert_eq!(status(&snap, "game_mode"), Status::Ok);
        snap.game_mode_auto = Some(0);
        assert_eq!(status(&snap, "game_mode"), Status::Issue);

        assert_eq!(status(&snap, "hags"), Status::Unknown);
        snap.hags_mode = Some(1);
        assert_eq!(status(&snap, "hags"), Status::Issue);
        snap.hags_mode = Some(2);
        assert_eq!(status(&snap, "hags"), Status::Ok);
        // Kernel caps win over the registry.
        snap.hags_caps = Some(HagsCaps { supported: true, enabled: false });
        assert_eq!(status(&snap, "hags"), Status::Issue);
        snap.hags_caps = Some(HagsCaps { supported: false, enabled: false });
        assert_eq!(status(&snap, "hags"), Status::NotApplicable);
        snap.hags_caps = None;

        assert_eq!(status(&snap, "windowed_optimizations"), Status::Ok);
        snap.os.build = 22631;
        assert_eq!(status(&snap, "windowed_optimizations"), Status::Issue);
        snap.dx_user_global_settings = Some(s("VRROptimizeEnable=0;SwapEffectUpgradeEnable=1;"));
        assert_eq!(status(&snap, "windowed_optimizations"), Status::Ok);
        snap.os.build = 19045;
        assert_eq!(status(&snap, "windowed_optimizations"), Status::NotApplicable);
    }

    #[test]
    fn memory_integrity_is_a_choice() {
        let mut snap = desktop();
        snap.hvci_enabled = Some(1);
        let f = evaluate_one(&snap, "memory_integrity").unwrap();
        assert_eq!(f.action, Action::Choose);
        assert_eq!(f.guide.choices.len(), 2);
    }

    #[test]
    fn memory_integrity_registry_vs_running() {
        let mut snap = desktop();
        snap.hvci_enabled = Some(1);
        snap.hvci_running = Some(true);
        assert_eq!(evaluate_one(&snap, "memory_integrity").unwrap().detected.as_deref(), Some("On"));
        // Switched on but not running: restart pending, or the PC can't run it.
        snap.hvci_running = Some(false);
        let f = evaluate_one(&snap, "memory_integrity").unwrap();
        assert_eq!(f.detected.as_deref(), Some("Switched on, not running yet"));
        assert!(f.fix.is_some(), "turning it back off is still offered");
        // Switched off but still running until a restart.
        snap.hvci_enabled = Some(0);
        snap.hvci_running = Some(true);
        let f = evaluate_one(&snap, "memory_integrity").unwrap();
        assert_eq!(f.status, Status::Ok);
        assert!(f.detected.unwrap().contains("until restart"));
    }

    #[test]
    fn drive_space() {
        let mut snap = desktop();
        snap.system_drive = Some(s("C:"));
        snap.drives = Some(vec![DriveInfo { mount: s("C:\\"), media: MediaType::Ssd, total_bytes: 1000, free_bytes: 50 }]);
        assert_eq!(status(&snap, "system_drive_space"), Status::Issue);
        snap.drives.as_mut().unwrap()[0].free_bytes = 300;
        assert_eq!(status(&snap, "system_drive_space"), Status::Ok);
    }

    #[test]
    fn network() {
        let mut snap = desktop();
        snap.network = Some(NetworkProbe { target: s("1.1.1.1"), samples_ms: vec![12.0, 13.0, 12.5, 14.0], failures: 0 });
        assert_eq!(status(&snap, "network_latency"), Status::Ok);
        snap.network = Some(NetworkProbe { target: s("1.1.1.1"), samples_ms: vec![12.0, 80.0, 15.0, 90.0], failures: 0 });
        assert_eq!(status(&snap, "network_latency"), Status::Issue);
        snap.network = Some(NetworkProbe { target: s("1.1.1.1"), samples_ms: vec![], failures: 6 });
        assert_eq!(status(&snap, "network_latency"), Status::Unknown);
    }
}
