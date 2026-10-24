//! The system monitor (R116): machine CPU, memory, GPU and network read
//! from `/proc` and `/sys`, the Statusline cell's text, the high states with
//! hysteresis, and the per-Pane CPU / memory of the popover. Pure parsing and
//! arithmetic over a proc / sys root (tests use fixtures); the app samples
//! on a worker thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Cumulative CPU jiffies from the first line of `/proc/stat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub total: u64,
    /// idle + iowait.
    pub idle: u64,
}

/// `/proc/stat`: the `cpu` line's times and the number of `cpuN` lines.
pub fn parse_stat(s: &str) -> Option<(CpuTimes, usize)> {
    let mut lines = s.lines();
    let first = lines.next()?;
    let mut f = first.split_whitespace();
    if f.next()? != "cpu" {
        return None;
    }
    let v: Vec<u64> = f.filter_map(|x| x.parse().ok()).collect();
    if v.len() < 5 {
        return None;
    }
    // user nice system idle iowait irq softirq steal (guest is in user).
    let total = v.iter().take(8).sum();
    let idle = v[3] + v[4];
    let cores = lines
        .take_while(|l| l.starts_with("cpu"))
        .filter(|l| l.as_bytes().get(3).is_some_and(u8::is_ascii_digit))
        .count();
    Some((CpuTimes { total, idle }, cores))
}

/// Busy share between two samples, 0–100.
pub fn cpu_percent(a: CpuTimes, b: CpuTimes) -> f64 {
    let total = b.total.saturating_sub(a.total);
    if total == 0 {
        return 0.0;
    }
    let idle = b.idle.saturating_sub(a.idle);
    100.0 * total.saturating_sub(idle) as f64 / total as f64
}

/// `/proc/meminfo` in KiB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mem {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl Mem {
    /// Used share, 0–100: (MemTotal − MemAvailable) / MemTotal.
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        100.0 * self.total.saturating_sub(self.available) as f64 / self.total as f64
    }
}

pub fn parse_meminfo(s: &str) -> Option<Mem> {
    let mut m = Mem::default();
    for line in s.lines() {
        let (k, rest) = line.split_once(':')?;
        let v: u64 = rest.split_whitespace().next()?.parse().ok()?;
        match k {
            "MemTotal" => m.total = v,
            "MemAvailable" => m.available = v,
            "SwapTotal" => m.swap_total = v,
            "SwapFree" => m.swap_free = v,
            _ => {}
        }
    }
    (m.total > 0).then_some(m)
}

/// `/proc/net/dev`: (interface, received bytes, sent bytes).
pub fn parse_net_dev(s: &str) -> Vec<(String, u64, u64)> {
    s.lines()
        .skip(2)
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let v: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            Some((name.trim().to_string(), *v.first()?, *v.get(8)?))
        })
        .collect()
}

/// The physical network interfaces under `sys` (`class/net/<if>/device`
/// exists, not `lo`): no bridges, veths or tunnels, so nothing is counted
/// twice.
pub fn physical_ifaces(sys: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(sys.join("class/net"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("device").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "lo")
        .collect();
    v.sort();
    v
}

/// Bytes per second as the cell shows it: `0B`, `312K`, `1.2M`, `18M`, `1.1G`.
pub fn fmt_rate(bps: f64) -> String {
    let b = bps.max(0.0);
    let (v, unit) = if b >= 1e9 {
        (b / 1e9, "G")
    } else if b >= 1e6 {
        (b / 1e6, "M")
    } else if b >= 1e3 {
        (b / 1e3, "K")
    } else {
        return format!("{}B", b.round() as u64);
    };
    if v < 10.0 {
        format!("{v:.1}{unit}")
    } else {
        format!("{}{unit}", v.round() as u64)
    }
}

/// One sample of the machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub cpu: f64,
    pub mem: f64,
    pub gpu: Option<f64>,
    /// Received / sent bytes per second on physical interfaces.
    pub rx: f64,
    pub tx: f64,
}

/// Which figure a part of the cell is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Cpu,
    Mem,
    Gpu,
    Net,
}

/// The cell's parts: (label, value, which); the label is empty for the
/// network part. Percentages padded to 3 so the width stays put.
pub fn cell_parts(s: &Sample) -> Vec<(&'static str, String, Part)> {
    let pct = |v: f64| format!("{:>3}%", v.round().clamp(0.0, 100.0) as u64);
    let mut v = vec![
        ("cpu", pct(s.cpu), Part::Cpu),
        ("mem", pct(s.mem), Part::Mem),
    ];
    if let Some(g) = s.gpu {
        v.push(("gpu", pct(g), Part::Gpu));
    }
    v.push((
        "",
        format!("↓{} ↑{}", fmt_rate(s.rx), fmt_rate(s.tx)),
        Part::Net,
    ));
    v
}

/// The Statusline cell (R116.2): `cpu  34% · mem  61% · gpu  12% · ↓1.2M
/// ↑312K`.
pub fn cell_text(s: &Sample) -> String {
    cell_parts(s)
        .into_iter()
        .map(|(l, v, _)| if l.is_empty() { v } else { format!("{l} {v}") })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// How loud a figure is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Normal,
    /// `wait` ink.
    High,
    /// `err` ink (memory near the OOM killer).
    Critical,
}

/// High states with hysteresis (R116.3): CPU / GPU high after 3 samples
/// ≥ 90 %, memory high at ≥ 85 % and critical at ≥ 95 %; back to normal
/// after 2 samples at least 5 points under the threshold.
#[derive(Clone, Debug, Default)]
pub struct Heat {
    cpu: Track,
    gpu: Track,
    mem: Track,
}

#[derive(Clone, Copy, Debug, Default)]
struct Track {
    level: Level,
    over: u32,
    under: u32,
}

impl Track {
    fn step(&mut self, v: f64, high: f64, need: u32, critical: Option<f64>) -> Level {
        if v >= high {
            self.over += 1;
            self.under = 0;
            if self.over >= need {
                self.level = Level::High;
            }
        } else {
            self.over = 0;
            if v <= high - 5.0 {
                self.under += 1;
                if self.under >= 2 {
                    self.level = Level::Normal;
                }
            } else {
                self.under = 0;
            }
        }
        match critical {
            Some(c) if v >= c => Level::Critical,
            _ => self.level,
        }
    }
}

impl Heat {
    /// Feed one sample; (cpu, mem, gpu) levels.
    pub fn update(&mut self, s: &Sample) -> (Level, Level, Level) {
        let cpu = self.cpu.step(s.cpu, 90.0, 3, None);
        let mem = self.mem.step(s.mem, 85.0, 1, Some(95.0));
        let gpu = match s.gpu {
            Some(g) => self.gpu.step(g, 90.0, 3, None),
            None => Level::Normal,
        };
        (cpu, mem, gpu)
    }
}

/// `/proc/loadavg`'s three averages.
pub fn parse_loadavg(s: &str) -> Option<[f64; 3]> {
    let mut f = s.split_whitespace().map(|x| x.parse::<f64>().ok());
    Some([f.next()??, f.next()??, f.next()??])
}

/// GPU busy % from `/sys/class/drm/card*/device/gpu_busy_percent` (AMD;
/// the highest card), when any card has it.
pub fn sysfs_gpu_busy(sys: &Path) -> Option<f64> {
    std::fs::read_dir(sys.join("class/drm"))
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("card") && !n.contains('-')
        })
        .filter_map(|e| {
            std::fs::read_to_string(e.path().join("device/gpu_busy_percent"))
                .ok()?
                .trim()
                .parse::<f64>()
                .ok()
        })
        .reduce(f64::max)
}

/// One process from `<proc>/<pid>/stat`: (session id, utime + stime
/// jiffies, rss pages). Fields are counted after the last `)` (the comm may
/// hold spaces and parentheses).
pub fn parse_pid_stat(s: &str) -> Option<(i32, u64, u64)> {
    let rest = &s[s.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // state ppid pgrp session … utime(11) stime(12) … rss(21).
    let session = f.get(3)?.parse().ok()?;
    let ut: u64 = f.get(11)?.parse().ok()?;
    let st: u64 = f.get(12)?.parse().ok()?;
    let rss: u64 = f.get(21)?.parse().ok()?;
    Some((session, ut + st, rss))
}

/// CPU jiffies and RSS pages per session id over every process under
/// `proc`, plus the totals of every process.
pub fn sessions(proc: &Path) -> (HashMap<i32, (u64, u64)>, (u64, u64)) {
    let mut by: HashMap<i32, (u64, u64)> = HashMap::new();
    let mut all = (0, 0);
    let Ok(dir) = std::fs::read_dir(proc) else {
        return (by, all);
    };
    let mut buf = String::new();
    for e in dir.flatten() {
        let name = e.file_name();
        if !name.as_encoded_bytes().iter().all(u8::is_ascii_digit) {
            continue;
        }
        buf.clear();
        let Ok(mut f) = std::fs::File::open(e.path().join("stat")) else {
            continue;
        };
        if std::io::Read::read_to_string(&mut f, &mut buf).is_err() {
            continue;
        }
        if let Some((sid, cpu, rss)) = parse_pid_stat(&buf) {
            let t = by.entry(sid).or_default();
            t.0 += cpu;
            t.1 += rss;
            all.0 += cpu;
            all.1 += rss;
        }
    }
    (by, all)
}

/// One row of the popover's BY PANE list.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneLoad {
    pub pane: String,
    /// % of one core (can pass 100).
    pub cpu: f64,
    pub rss_bytes: u64,
}

/// Per-Pane CPU % (of one core) and memory between two session scans.
/// `panes` = (Pane id, its process's pid = session id); `hz` = clock ticks
/// per second; `secs` between the scans. Sorted by CPU, then memory.
pub fn pane_loads(
    panes: &[(String, i32)],
    before: &HashMap<i32, (u64, u64)>,
    now: &HashMap<i32, (u64, u64)>,
    hz: f64,
    secs: f64,
    page: u64,
) -> Vec<PaneLoad> {
    let mut v: Vec<PaneLoad> = panes
        .iter()
        .filter_map(|(pane, sid)| {
            let (cpu, rss) = *now.get(sid)?;
            let prev = before.get(sid).map_or(cpu, |p| p.0);
            Some(PaneLoad {
                pane: pane.clone(),
                cpu: 100.0 * cpu.saturating_sub(prev) as f64 / hz / secs.max(0.001),
                rss_bytes: rss * page,
            })
        })
        .collect();
    v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.rss_bytes.cmp(&a.rss_bytes)));
    v
}

/// Memory as the popover shows it: `812M`, `1.8G`.
pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1}G", b / 1e9)
    } else if b >= 1e6 {
        format!("{}M", (b / 1e6).round() as u64)
    } else {
        format!("{}K", (b / 1e3).round() as u64)
    }
}

/// Clock ticks per second and the page size (for `/proc/<pid>/stat`).
pub fn clock_and_page() -> (f64, u64) {
    // SAFETY: sysconf reads constants.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (hz.max(1) as f64, page.max(1) as u64)
}

/// Where the monitor reads (`RADE_PROC_ROOT` / `RADE_SYS_ROOT` in tests).
#[derive(Clone, Debug)]
pub struct Roots {
    pub proc: PathBuf,
    pub sys: PathBuf,
}

impl Roots {
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Self {
        Self {
            proc: get("RADE_PROC_ROOT").map_or_else(|| "/proc".into(), PathBuf::from),
            sys: get("RADE_SYS_ROOT").map_or_else(|| "/sys".into(), PathBuf::from),
        }
    }

    /// Real `/proc` (the NVIDIA query runs only then).
    pub fn real(&self) -> bool {
        self.proc == Path::new("/proc")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT_A: &str =
        "cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 0 1 1 1 0 0 0\ncpu1 1 0 1 1 1 0 0 0\nintr 1\n";
    // +1000 jiffies, of which 660 idle/iowait → 34 % busy.
    const STAT_B: &str =
        "cpu  300 0 240 1300 160 0 0 0 0 0\ncpu0 1 0 1 1 1 0 0 0\ncpu1 1 0 1 1 1 0 0 0\n";

    fn fixture(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rade-sysmon-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn r116_a_snapshot_formats_cpu_mem_net_without_gpu() {
        let (a, cores) = parse_stat(STAT_A).unwrap();
        let (b, _) = parse_stat(STAT_B).unwrap();
        assert_eq!(cores, 2);
        let cpu = cpu_percent(a, b);
        assert!((cpu - 34.0).abs() < 0.01, "{cpu}");
        let mem = parse_meminfo(
            "MemTotal:       1000000 kB\nMemFree:  10 kB\nMemAvailable:    390000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        assert!((mem.percent() - 61.0).abs() < 0.01);
        let dev = "Inter-|   Receive\n face |bytes\n    lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0\n\
                   docker0: 5000 1 0 0 0 0 0 0 5000 0 0 0 0 0 0 0\n  enp1s0: 2400000 9 0 0 0 0 0 0 624000 3 0 0 0 0 0 0\n";
        let net = parse_net_dev(dev);
        assert_eq!(net[2], ("enp1s0".into(), 2_400_000, 624_000));
        // Only physical interfaces count.
        let sys = fixture("sys");
        for i in ["lo", "docker0", "veth1", "enp1s0"] {
            std::fs::create_dir_all(sys.join("class/net").join(i)).unwrap();
        }
        std::fs::create_dir_all(sys.join("class/net/enp1s0/device")).unwrap();
        assert_eq!(physical_ifaces(&sys), ["enp1s0"]);
        // +2.4 MB / +624 KB over 2 s.
        let s = Sample {
            cpu,
            mem: mem.percent(),
            gpu: None,
            rx: 2_400_000.0 / 2.0,
            tx: 624_000.0 / 2.0,
        };
        assert_eq!(cell_text(&s), "cpu  34% · mem  61% · ↓1.2M ↑312K");
        let g = Sample {
            gpu: Some(7.4),
            ..s.clone()
        };
        assert_eq!(
            cell_text(&g),
            "cpu  34% · mem  61% · gpu   7% · ↓1.2M ↑312K"
        );
        assert_eq!(fmt_rate(0.0), "0B");
        assert_eq!(fmt_rate(18_400_000.0), "18M");
        assert_eq!(fmt_rate(1_100_000_000.0), "1.1G");
        // AMD's sysfs busy %.
        std::fs::create_dir_all(sys.join("class/drm/card1/device")).unwrap();
        std::fs::create_dir_all(sys.join("class/drm/card1-DP-1")).unwrap();
        assert_eq!(sysfs_gpu_busy(&sys), None);
        std::fs::write(sys.join("class/drm/card1/device/gpu_busy_percent"), "12\n").unwrap();
        assert_eq!(sysfs_gpu_busy(&sys), Some(12.0));
        std::fs::remove_dir_all(sys).unwrap();
    }

    #[test]
    fn r116_b_high_states_have_hysteresis() {
        let mut h = Heat::default();
        let cpu = |v| Sample {
            cpu: v,
            mem: 50.0,
            ..Default::default()
        };
        let levels: Vec<Level> = [92.0, 93.0, 95.0, 88.0, 84.0, 84.0]
            .iter()
            .map(|v| h.update(&cpu(*v)).0)
            .collect();
        use Level::*;
        assert_eq!(levels, [Normal, Normal, High, High, High, Normal]);
        let mut h = Heat::default();
        let mem = |v| Sample {
            mem: v,
            ..Default::default()
        };
        assert_eq!(h.update(&mem(86.0)).1, High);
        assert_eq!(h.update(&mem(96.0)).1, Critical);
        assert_eq!(h.update(&mem(82.0)).1, High);
        assert_eq!(h.update(&mem(79.0)).1, High);
        assert_eq!(h.update(&mem(79.0)).1, Normal);
    }

    #[test]
    fn r116_d_pane_loads_from_sessions() {
        let proc = fixture("proc");
        let stat = |pid: i32, sid: i32, ticks: u64, rss: u64| {
            let d = proc.join(pid.to_string());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("stat"),
                format!("{pid} (a (b) c) S 1 {sid} {sid} 0 -1 0 0 0 0 0 {ticks} 0 0 0 20 0 1 0 0 0 {rss} 0"),
            )
            .unwrap();
        };
        stat(100, 100, 10, 100); // tests' shell
        stat(101, 100, 10, 900); // its cargo
        stat(200, 200, 5, 50); // planner
        stat(300, 300, 1, 10); // outside RADE
        std::fs::create_dir_all(proc.join("self")).unwrap();
        let (before, all) = sessions(&proc);
        assert_eq!(before[&100], (20, 1000));
        assert_eq!(all, (26, 1060));
        stat(101, 100, 210, 900); // +200 ticks in 2 s at 100 Hz = 100 %
        let (now, _) = sessions(&proc);
        let panes = [("p_tests".to_string(), 100), ("p_planner".to_string(), 200)];
        let rows = pane_loads(&panes, &before, &now, 100.0, 2.0, 4096);
        assert_eq!(rows[0].pane, "p_tests");
        assert!((rows[0].cpu - 100.0).abs() < 0.01);
        assert_eq!(rows[0].rss_bytes, 1000 * 4096);
        assert_eq!(rows[1].pane, "p_planner");
        assert_eq!(rows[1].cpu, 0.0);
        assert_eq!(fmt_bytes(4_096_000), "4M");
        assert_eq!(fmt_bytes(1_800_000_000), "1.8G");
        assert_eq!(
            parse_loadavg("3.20 2.90 2.50 2/2000 1"),
            Some([3.2, 2.9, 2.5])
        );
        std::fs::remove_dir_all(proc).unwrap();
    }
}
