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

const KILL_AFTER: Duration = Duration::from_secs(2);
const CRITICAL_COOLDOWN: Duration = Duration::from_secs(10);
const EVENTS: usize = 50;

pub fn human(bytes: u64) -> String {
    let mb = bytes as f64 / (1u64 << 20) as f64;
    if mb >= 1024.0 {
        format!("{:.1}G", mb / 1024.0)
    } else {
        format!("{mb:.0}M")
    }
}

struct Pending {
    pid: u32,
    start: u64,
    at: Instant,
}

pub struct Actor {
    pub auto: bool,
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
    pub fn new(auto: bool, grace: Duration) -> Self {
        let log = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/aimon"));
        if let Some(dir) = &log {
            let _ = std::fs::create_dir_all(dir);
        }
        Self {
            auto,
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
                    "pressão {:?}: RAM {}/{} swap {}",
                    mem.pressure,
                    human(mem.used),
                    human(mem.total),
                    human(mem.swap_used)
                );
                notify(&msg);
                self.note(msg);
            }
            self.last_pressure = mem.pressure;
        }
        let orphans: HashSet<(u32, u64)> = view
            .rows()
            .filter(|r| r.orphan && r.kind == Kind::Tool)
            .map(|r| (r.pid, r.start))
            .collect();
        self.first_seen.retain(|k, _| orphans.contains(k));
        for k in &orphans {
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
        let mut due: Vec<(Row, &str)> = view
            .rows()
            .filter(|r| r.orphan && r.kind == Kind::Tool)
            .filter(|r| {
                self.first_seen
                    .get(&(r.pid, r.start))
                    .is_some_and(|t| t.elapsed() >= grace)
            })
            .map(|r| (r.clone(), "auto: órfão"))
            .collect();
        // ponytail: critical kills only the heaviest target per cooldown, so a running test or
        // build is lost only while pressure stays critical.
        if critical && let Some(r) = view.targets().into_iter().max_by_key(|r| r.rss) {
            due.push((r, "auto: pressão crítica"));
        }
        for (r, why) in due {
            self.kill_tree(s, procs, &r, why);
        }
        self.flush();
    }

    /// TERM to `row` and every descendant; KILL follows in `KILL_AFTER` for any survivor.
    pub fn kill_tree(&mut self, s: &Sampler, procs: &[Proc], row: &Row, why: &str) {
        let mut stack = vec![(row.pid, row.start)];
        let mut total = 0u64;
        let mut seen = HashSet::new();
        while let Some((pid, start)) = stack.pop() {
            if !seen.insert(pid) {
                continue;
            }
            if s.signal(pid, start, Signal::Term) {
                self.pending.push(Pending {
                    pid,
                    start,
                    at: Instant::now(),
                });
            }
            for c in procs.iter().filter(|p| p.ppid == pid) {
                total += c.rss;
                stack.push((c.pid, c.start));
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

    /// One notification per batch of kills, not one per process.
    pub fn flush(&mut self) {
        match self.batch.len() {
            0 => {}
            1 => notify(&self.batch[0]),
            n => notify(&format!("{n} processos mortos · {}", self.batch[0])),
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

/// Native macOS banner. Runs off-thread so a slow osascript never stalls the loop.
fn notify(msg: &str) {
    let script = format!(
        "display notification \"{}\" with title \"aimon\"",
        msg.replace(['"', '\\'], "'")
    );
    std::thread::spawn(move || {
        let _ = Command::new("osascript").args(["-e", &script]).status();
    });
}

#[cfg(test)]
mod tests {
    use super::human;

    #[test]
    fn sizes() {
        assert_eq!(human(300 << 20), "300M");
        assert_eq!(human(1536 << 20), "1.5G");
    }
}
