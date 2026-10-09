//! Reads the machine: processes of the current user, RAM, swap, the kernel's memory pressure
//! level and which tmux session owns which pane. Only what the view needs is refreshed.
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use sysinfo::{
    MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, Uid, UpdateKind,
};

const TMUX_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    /// Lowercased command line, read once per process.
    pub cmd: String,
    pub rss: u64,
    pub cpu: f32,
    pub start: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pressure {
    #[default]
    Normal,
    Warn,
    Critical,
}

impl Pressure {
    /// `kern.memorystatus_vm_pressure_level`: 1 normal, 2 warn, 4 critical.
    pub fn from_level(level: i64) -> Self {
        match level {
            4.. => Pressure::Critical,
            2..=3 => Pressure::Warn,
            _ => Pressure::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mem {
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub pressure: Pressure,
}

pub struct Sampler {
    sys: System,
    uid: Option<Uid>,
    me: u32,
    pub tmux: HashMap<u32, String>,
    tmux_at: Option<Instant>,
}

impl Sampler {
    pub fn new() -> Self {
        let mut s = Self {
            sys: System::new(),
            uid: None,
            me: std::process::id(),
            tmux: HashMap::new(),
            tmux_at: None,
        };
        s.refresh();
        s.uid = s
            .sys
            .process(Pid::from_u32(s.me))
            .and_then(|p| p.user_id().cloned());
        s
    }

    fn refresh(&mut self) {
        let kind = ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram().with_swap());
    }

    pub fn sample(&mut self) -> (Vec<Proc>, Mem) {
        self.refresh();
        if self.tmux_at.is_none_or(|t| t.elapsed() >= TMUX_EVERY) {
            self.tmux = tmux_panes();
            self.tmux_at = Some(Instant::now());
        }
        let procs = self
            .sys
            .processes()
            .values()
            .filter(|p| p.pid().as_u32() != self.me)
            .filter(|p| self.uid.is_none() || p.user_id() == self.uid.as_ref())
            .map(|p| Proc {
                pid: p.pid().as_u32(),
                ppid: p.parent().map_or(0, Pid::as_u32),
                name: p.name().to_string_lossy().into_owned(),
                cmd: cmdline(p.cmd()),
                rss: p.memory(),
                cpu: p.cpu_usage(),
                start: p.start_time(),
            })
            .collect();
        let mem = Mem {
            used: self.sys.used_memory(),
            total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            pressure: pressure(),
        };
        (procs, mem)
    }

    /// Sends `sig` only if `pid` is still the process that started at `start` (no pid reuse).
    pub fn signal(&self, pid: u32, start: u64, sig: Signal) -> bool {
        self.sys
            .process(Pid::from_u32(pid))
            .filter(|p| p.start_time() == start)
            .and_then(|p| p.kill_with(sig))
            .unwrap_or(false)
    }

    pub fn alive(&self, pid: u32, start: u64) -> bool {
        self.sys
            .process(Pid::from_u32(pid))
            .is_some_and(|p| p.start_time() == start)
    }
}

const CMD_MAX: usize = 256;

/// Lowercased args, capped. A process that retitles itself (`npm exec …`) makes the kernel
/// hand back its environment as extra args, so everything from the first `KEY=value` is dropped.
pub fn cmdline(args: &[std::ffi::OsString]) -> String {
    let mut out = String::new();
    for a in args.iter().map(|a| a.to_string_lossy()) {
        let env = a.split_once('=').is_some_and(|(k, _)| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        });
        if env || out.len() >= CMD_MAX {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&a.to_lowercase());
    }
    out.truncate(out.floor_char_boundary(CMD_MAX));
    out
}

fn pressure() -> Pressure {
    use sysctl::Sysctl;
    let level = sysctl::Ctl::new("kern.memorystatus_vm_pressure_level")
        .and_then(|c| c.value())
        .ok()
        .and_then(|v| match v {
            sysctl::CtlValue::Int(i) => Some(i64::from(i)),
            sysctl::CtlValue::Uint(i) => Some(i64::from(i)),
            _ => None,
        })
        .unwrap_or(1);
    Pressure::from_level(level)
}

/// pane pid -> session name. Empty when tmux is not running.
fn tmux_panes() -> HashMap<u32, String> {
    let out = Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{pane_pid}\t#{session_name}"])
        .output();
    let Ok(out) = out else {
        return HashMap::new();
    };
    parse_panes(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_panes(text: &str) -> HashMap<u32, String> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter_map(|(pid, name)| Some((pid.trim().parse().ok()?, name.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_levels() {
        assert_eq!(Pressure::from_level(1), Pressure::Normal);
        assert_eq!(Pressure::from_level(2), Pressure::Warn);
        assert_eq!(Pressure::from_level(4), Pressure::Critical);
    }

    #[test]
    fn env_is_not_cmd() {
        let args: Vec<std::ffi::OsString> = ["npm exec x mcp", "HOME=/u", "PATH=/claude/versions/"]
            .map(Into::into)
            .to_vec();
        assert_eq!(cmdline(&args), "npm exec x mcp");
    }

    #[test]
    fn panes_parse() {
        let m = parse_panes("123\tprojects\nbad line\n456\twork\n");
        assert_eq!(m.get(&123).map(String::as_str), Some("projects"));
        assert_eq!(m.len(), 2);
    }
}
