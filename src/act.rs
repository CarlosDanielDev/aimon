//! Side effects: killing (TERM, then KILL after a grace), the auto policy, the kill log and the
//! native notification. Every kill goes through `kill_tree` so the log and the alert never miss one.
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sysinfo::Signal;

use crate::classify::{Kind, Row, View};
use crate::sys::{Mem, Pressure, Proc, Sampler};
use crate::theme::human;

const KILL_AFTER: Duration = Duration::from_secs(2);
const CRITICAL_COOLDOWN: Duration = Duration::from_secs(10);
const EVENTS: usize = 50;

/// The graceful signal: TERM everywhere it exists; Windows only has Kill.
#[cfg(unix)]
fn term_signal() -> Signal {
    Signal::Term
}

#[cfg(not(unix))]
fn term_signal() -> Signal {
    Signal::Kill
}

struct Pending {
    pid: u32,
    start: u64,
    at: Instant,
}

pub struct Actor {
    pub auto: bool,
    pub auto_dev: bool,
    grace: Duration,
    first_seen: HashMap<(u32, u64), Instant>,
    pending: Vec<Pending>,
    last_critical: Option<Instant>,
    last_pressure: Pressure,
    log: Option<PathBuf>,
    pub events: VecDeque<String>,
    batch: Vec<String>,
}

impl Actor {
    pub fn new(auto: bool, grace: Duration, auto_dev: bool) -> Self {
        let log = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/aimon"));
        if let Some(dir) = &log {
            let _ = std::fs::create_dir_all(dir);
        }
        Self {
            auto,
            auto_dev,
            grace,
            first_seen: HashMap::new(),
            pending: Vec::new(),
            last_critical: None,
            last_pressure: Pressure::Normal,
            log: log.map(|d| d.join("kills.log")),
            events: VecDeque::new(),
            batch: Vec::new(),
        }
    }

    pub fn note(&mut self, msg: String) {
        if self.events.len() == EVENTS {
            self.events.pop_back();
        }
        self.events.push_front(msg);
    }

    /// One pass: escalate pending kills, alert on pressure changes, run the auto policy.
    pub fn tick(&mut self, s: &Sampler, procs: &[Proc], view: &View, mem: &Mem) {
        self.escalate(s);
        if mem.pressure != self.last_pressure {
            if mem.pressure != Pressure::Normal {
                let msg = format!(
                    "pressure {}: RAM {}/{} swap {}",
                    mem.pressure.label(),
                    human(mem.used),
                    human(mem.total),
                    human(mem.swap_used)
                );
                notify(&msg);
                self.note(msg);
            }
            self.last_pressure = mem.pressure;
        }
        // Rule A orphans: exactly the rows the panic popup pre-marks. One collection drives both
        // the grace bookkeeping and the auto-kill list, so the two can never drift apart.
        let orphans: Vec<&Row> = view.rows().filter(|r| self.orphan_killable(r)).collect();
        let seen: HashSet<(u32, u64)> = orphans.iter().map(|r| (r.pid, r.start)).collect();
        self.first_seen.retain(|k, _| seen.contains(k));
        for k in &seen {
            self.first_seen.entry(*k).or_insert_with(Instant::now);
        }
        if !self.auto {
            return;
        }
        let critical = mem.pressure == Pressure::Critical
            && self
                .last_critical
                .is_none_or(|t| t.elapsed() >= CRITICAL_COOLDOWN);
        if critical {
            self.last_critical = Some(Instant::now());
        }
        let grace = self.grace;
        let mut due: Vec<(&Row, &str)> = orphans
            .into_iter()
            .filter(|r| {
                self.first_seen
                    .get(&(r.pid, r.start))
                    .is_some_and(|t| t.elapsed() >= grace)
            })
            .map(|r| (r, "auto: orphan"))
            .collect();
        // ponytail: critical kills only the heaviest target per cooldown, so a running test or
        // build is lost only while pressure stays critical.
        if critical
            && let Some(r) = view
                .rows()
                .filter(|r| r.why.is_some())
                .max_by_key(|r| r.rss)
        {
            due.push((r, "auto: critical pressure"));
        }
        for (r, why) in &due {
            self.kill_tree(s, procs, r, why);
        }
        self.flush();
    }

    /// TERM to `row` and every descendant; KILL follows in `KILL_AFTER` for any survivor.
    pub fn kill_tree(&mut self, s: &Sampler, procs: &[Proc], row: &Row, why: &str) {
        let kids: HashMap<u32, Vec<&Proc>> = procs.iter().fold(HashMap::new(), |mut m, p| {
            m.entry(p.ppid).or_default().push(p);
            m
        });
        let mut stack = vec![(row.pid, row.start)];
        let mut total = 0u64;
        let mut seen = HashSet::new();
        while let Some((pid, start)) = stack.pop() {
            if !seen.insert(pid) {
                continue;
            }
            if s.signal(pid, start, term_signal()) {
                self.pending.push(Pending {
                    pid,
                    start,
                    at: Instant::now(),
                });
            }
            if let Some(children) = kids.get(&pid) {
                for c in children {
                    total += c.rss;
                    stack.push((c.pid, c.start));
                }
            }
        }
        self.first_seen.remove(&(row.pid, row.start));
        let msg = format!(
            "{why} · {} {} ({})",
            row.pid,
            row.label,
            human(row.rss + total)
        );
        self.log_line(&msg);
        self.batch.push(msg.clone());
        self.note(msg);
    }

    /// What the grace-period reaper targets: orphaned tool workers always, orphaned runtimes
    /// only when --auto-dev opted in.
    fn orphan_killable(&self, r: &Row) -> bool {
        r.is_orphan_tool() || (self.auto_dev && r.kind == Kind::Dev && r.orphan)
    }

    fn escalate(&mut self, s: &Sampler) {
        self.pending.retain(|p| {
            if p.at.elapsed() < KILL_AFTER {
                return true;
            }
            if s.alive(p.pid, p.start) {
                s.signal(p.pid, p.start, Signal::Kill);
            }
            false
        });
    }

    /// Finish pending kills on the way out: honor the remaining TERM grace, then KILL any
    /// survivor. Called where no next tick exists to do it. Refreshes the sampler first so a
    /// just-reused pid is never mistaken for the process that was killed.
    pub fn escalate_now(&mut self, s: &mut Sampler) {
        s.refresh();
        for p in std::mem::take(&mut self.pending) {
            let due = p.at + KILL_AFTER;
            while Instant::now() < due && s.alive(p.pid, p.start) {
                std::thread::sleep(Duration::from_millis(50));
            }
            if s.alive(p.pid, p.start) {
                s.signal(p.pid, p.start, Signal::Kill);
            }
        }
    }

    /// One notification per batch of kills, not one per process.
    pub fn flush(&mut self) {
        match self.batch.len() {
            0 => {}
            1 => notify(&self.batch[0]),
            n => notify(&format!("{n} processes killed · {}", self.batch[0])),
        }
        self.batch.clear();
    }

    fn log_line(&self, msg: &str) {
        let Some(path) = &self.log else { return };
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{secs}\t{msg}");
        }
    }
}

/// OS banner. Runs off-thread so a slow notifier never stalls the loop.
#[cfg(target_os = "macos")]
fn notify(msg: &str) {
    let script = format!(
        "display notification \"{}\" with title \"aimon\"",
        msg.replace(['"', '\\'], "'")
    );
    std::thread::spawn(move || {
        let _ = Command::new("osascript").args(["-e", &script]).status();
    });
}

/// Linux desktop notification. Silently absent when notify-send is not installed.
#[cfg(target_os = "linux")]
fn notify(msg: &str) {
    let msg = msg.to_string(); // owned: the banner thread outlives this borrow
    std::thread::spawn(move || {
        let _ = Command::new("notify-send")
            .args(["-a", "aimon", "aimon", &msg])
            .status();
    });
}

/// Windows toast via PowerShell (WinRT from PS 5+). Single-quoted PS strings, so the only
/// escaping needed is doubling the quotes inside `msg`.
#[cfg(target_os = "windows")]
fn notify(msg: &str) {
    let script = format!(
        r#"[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); $x.GetElementsByTagName('text').Item(0).AppendChild($x.CreateTextNode('aimon')) > $null; $x.GetElementsByTagName('text').Item(1).AppendChild($x.CreateTextNode('{}')) > $null; [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('aimon').Show([Windows.UI.Notifications.ToastNotification]::new($x))"#,
        msg.replace('\'', "''")
    );
    std::thread::spawn(move || {
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status();
    });
}

/// No toast mechanism known: the event bar and the log still carry every kill.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn notify(msg: &str) {
    let _ = msg;
}
