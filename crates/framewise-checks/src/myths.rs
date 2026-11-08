//! The Myths page: what FrameWise deliberately doesn't do, and why.

use crate::model::Link;
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct Myth {
    pub id: &'static str,
    pub claim: &'static str,
    pub verdict: &'static str,
    pub explanation: &'static str,
    pub links: Vec<Link>,
    /// The one documented exception: off by default, advanced only.
    pub advanced_exception: bool,
}

fn link(label: &str, url: &str) -> Link {
    Link { label: label.to_string(), url: url.to_string() }
}

pub fn myths() -> Vec<Myth> {
    vec![
        Myth {
            id: "ram_cleaners",
            claim: "“Freeing RAM” makes games faster",
            verdict: "FrameWise doesn't clean RAM",
            explanation: "Windows deliberately fills spare memory with a cache of recently used files (the standby list), and hands that memory to any app that needs it instantly. A RAM cleaner empties that cache, which looks good on a graph but forces Windows to reload those files from disk, which can cause more stutter, not less. Unused RAM is wasted RAM.",
            links: vec![
                link("Microsoft: Memory management in Windows", "https://learn.microsoft.com/en-us/windows/win32/memory/memory-management"),
                link("Sysinternals RAMMap (see the standby list yourself)", "https://learn.microsoft.com/en-us/sysinternals/downloads/rammap"),
            ],
            advanced_exception: false,
        },
        Myth {
            id: "registry_cleaners",
            claim: "Cleaning the registry speeds up your PC",
            verdict: "FrameWise doesn't clean the registry",
            explanation: "Leftover registry entries from uninstalled apps take a few kilobytes and are never read, so removing them has no measurable effect on speed. Deleting the wrong entry, though, can break apps or stop Windows from starting. Microsoft doesn't support the use of registry cleaners.",
            links: vec![link("Microsoft support policy on registry cleaners", "https://support.microsoft.com/kb/2563254")],
            advanced_exception: false,
        },
        Myth {
            id: "service_stripping",
            claim: "Disabling Windows services frees up resources",
            verdict: "FrameWise doesn't strip services",
            explanation: "Idle Windows services use almost no CPU and their memory is paged out when needed. 'Debloat' scripts that disable dozens of services at once regularly break Windows Update, the Microsoft Store, Bluetooth, printing or Xbox game features, and give no FPS gain you can measure. FrameWise only changes settings with a measurable effect.",
            links: vec![],
            advanced_exception: false,
        },
        Myth {
            id: "tcp_and_priority",
            claim: "TCP optimisers and process-priority tweaks lower ping",
            verdict: "FrameWise doesn't apply network or priority hacks",
            explanation: "Ping is set by the distance to the server and the quality of your connection, not by Windows' TCP settings, which already tune themselves. Forcing games to 'High' or 'Realtime' priority can starve audio, input and the game's own helper threads. Game Mode already gives games the priority that helps.",
            links: vec![],
            advanced_exception: false,
        },
        Myth {
            id: "standby_list",
            claim: "Clearing the standby list fixes stutter",
            verdict: "Sometimes, on some PCs: an advanced, opt-in exception",
            explanation: "On PCs with 8–16 GB of RAM, a few games have stuttered when the standby list filled up, and clearing it helped. On most PCs it does nothing or makes things worse, for the same reason RAM cleaners do. FrameWise offers it below as an advanced, one-off action, off by default. Benchmark a game before and after, and only keep using it if benchmark mode shows a real gain on your PC.",
            links: vec![link("Sysinternals RAMMap", "https://learn.microsoft.com/en-us/sysinternals/downloads/rammap")],
            advanced_exception: true,
        },
    ]
}
