//! What the POST screen reports. Everything is real data read from the
//! kernel; `fake()` gives fixed values for golden-image tests.

use std::fs;

#[derive(Clone, Debug)]
pub struct SysInfo {
    pub model: String,
    pub cpu: String,
    pub mem_kb: u64,
    pub video: String,
    pub storage: Vec<String>,
    pub input: String,
    pub network: Vec<String>,
}

fn read(p: &str) -> String {
    fs::read_to_string(p)
        .map(|s| s.trim_end_matches(['\0', '\n']).to_string())
        .unwrap_or_default()
}

fn cpu_name(part: &str) -> &'static str {
    match part {
        "0xd0b" => "ARM Cortex-A76",
        "0xd08" => "ARM Cortex-A72",
        "0xd03" => "ARM Cortex-A53",
        "0xd05" => "ARM Cortex-A55",
        "0xd07" => "ARM Cortex-A57",
        "0xd0c" => "ARM Neoverse-N1",
        "0xd40" => "ARM Neoverse-V1",
        _ => "ARM 64-bit",
    }
}

pub fn probe(video_desc: &str) -> SysInfo {
    let cpuinfo = read("/proc/cpuinfo");
    let cores = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count()
        .max(1);
    let part = cpuinfo
        .lines()
        .find(|l| l.starts_with("CPU part"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let mhz = read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .parse::<u64>()
        .unwrap_or(0)
        / 1000;
    let mut cpu = format!("{} {cores}-core", cpu_name(&part));
    if mhz > 0 {
        cpu.push_str(&format!(" @ {mhz} MHz"));
    }
    let mem_kb = read("/proc/meminfo")
        .lines()
        .find(|l| l.starts_with("MemTotal"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let model = {
        let m = read("/proc/device-tree/model");
        if m.is_empty() {
            let v = read("/sys/class/dmi/id/product_name");
            if v.is_empty() {
                "Generic ARM64 computer".to_string()
            } else {
                v
            }
        } else {
            m
        }
    };
    let compat = read("/proc/device-tree/compatible");
    let gpu = if compat.contains("bcm2712") {
        "VideoCore VII"
    } else if compat.contains("bcm2711") {
        "VideoCore VI"
    } else if fs::read_dir("/sys/bus/virtio/drivers/virtio_gpu")
        .map(|d| d.count() > 1)
        .unwrap_or(false)
    {
        "Virtio GPU"
    } else {
        "Display adapter"
    };
    let video = format!("{gpu}, {video_desc}");

    let mut storage = Vec::new();
    if let Ok(rd) = fs::read_dir("/sys/block") {
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for n in names {
            let kind = if n.starts_with("mmcblk") {
                "SD card"
            } else if n.starts_with("nvme") {
                "NVMe disk"
            } else if n.starts_with("sd") {
                "USB disk"
            } else if n.starts_with("vd") {
                "Virtual disk"
            } else {
                continue;
            };
            let sectors: u64 = read(&format!("/sys/block/{n}/size")).parse().unwrap_or(0);
            if sectors == 0 {
                continue;
            }
            let gb = (sectors * 512) as f64 / 1e9;
            storage.push(format!("{kind} {:.0} GB", gb.max(1.0)));
        }
    }

    let devices = read("/proc/bus/input/devices");
    let (mut kbd, mut mouse, mut pads) = (0, 0, 0);
    for block in devices.split("\n\n") {
        let h = block
            .lines()
            .find(|l| l.starts_with("H: Handlers="))
            .unwrap_or("");
        let ev = block
            .lines()
            .find(|l| l.starts_with("B: EV="))
            .unwrap_or("");
        if h.contains("js") {
            pads += 1;
        } else if h.contains("mouse") {
            mouse += 1;
        } else if h.contains("kbd") && ev.contains("120013") {
            kbd += 1; // EV_SYN|EV_KEY|EV_MSC|EV_LED|EV_REP: a real keyboard
        }
    }
    let plural = |n: i32, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    let mut input = vec![plural(kbd, "keyboard"), plural(mouse, "mouse")];
    if mouse != 1 {
        input[1] = format!("{mouse} mice");
    }
    if pads > 0 {
        input.push(plural(pads, "game controller"));
    }

    let mut network = Vec::new();
    if let Ok(rd) = fs::read_dir("/sys/class/net") {
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        if names
            .iter()
            .any(|n| n.starts_with("eth") || n.starts_with("en"))
        {
            network.push("Ethernet".to_string());
        }
        if names.iter().any(|n| n.starts_with("wl")) {
            network.push("Wi-Fi".to_string());
        }
    }
    if fs::read_dir("/sys/class/bluetooth")
        .map(|d| d.count() > 0)
        .unwrap_or(false)
    {
        network.push("Bluetooth".to_string());
    }

    SysInfo {
        model,
        cpu,
        mem_kb,
        video,
        storage,
        input: input.join(", "),
        network,
    }
}

pub fn fake() -> SysInfo {
    SysInfo {
        model: "Raspberry Pi 5 Model B Rev 1.0".into(),
        cpu: "ARM Cortex-A76 4-core @ 2400 MHz".into(),
        mem_kb: 8_245_632,
        video: "VideoCore VII, HDMI-A-1 1920x1080".into(),
        storage: vec!["SD card 64 GB".into()],
        input: "1 keyboard, 1 mouse".into(),
        network: vec!["Ethernet".into(), "Wi-Fi".into(), "Bluetooth".into()],
    }
}
