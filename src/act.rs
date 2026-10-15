//! Side effects: killing (TERM, then KILL after a grace), the auto policy, the kill log and the
//! native notification. Every kill goes through `kill_tree` so the log and the alert never miss one.
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
#[cfg(not(test))]
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

    /// Tests: no log file and no banners — receipts are asserted through events and batch.
    #[cfg(test)]
    pub fn cold(auto: bool, grace: Duration, auto_dev: bool) -> Self {
        Self {
            auto,
            auto_dev,
            grace,
            first_seen: HashMap::new(),
            pending: Vec::new(),
            last_critical: None,
            last_pressure: Pressure::Normal,
            log: None,
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
    /// `now` is injected so tests can walk time without sleeping.
    pub fn tick(&mut self, s: &Sampler, procs: &[Proc], view: &View, mem: &Mem, now: Instant) {
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
            self.first_seen.entry(*k).or_insert(now);
        }
        if !self.auto {
            return;
        }
        let critical = mem.pressure == Pressure::Critical
            && self
                .last_critical
                .is_none_or(|t| now.duration_since(t) >= CRITICAL_COOLDOWN);
        if critical {
            self.last_critical = Some(now);
        }
        let grace = self.grace;
        let mut due: Vec<(&Row, &str)> = orphans
            .into_iter()
            .filter(|r| {
                self.first_seen
                    .get(&(r.pid, r.start))
                    .is_some_and(|t| now.duration_since(*t) >= grace)
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
        let (targets, child_rss) = tree_targets(procs, row);
        for (pid, start) in targets {
            if s.signal(pid, start, term_signal()) {
                self.pending.push(Pending {
                    pid,
                    start,
                    at: Instant::now(),
                });
            }
        }
        self.first_seen.remove(&(row.pid, row.start));
        let msg = format!(
            "{why} · {} {} ({})",
            row.pid,
            row.label,
            human(row.rss + child_rss)
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

    /// The re-kill is user-visible like every other kill path: a receipt in the event bar.
    fn escalate(&mut self, s: &Sampler) {
        let (due, keep): (Vec<Pending>, Vec<Pending>) = self
            .pending
            .drain(..)
            .partition(|p| p.at.elapsed() >= KILL_AFTER);
        self.pending = keep;
        for p in &due {
            self.finish(s, p);
        }
    }

    /// Finish pending kills on the way out: honor the remaining TERM grace, then KILL any
    /// survivor. Called where no next tick exists to do it. Refreshes the sampler first so a
    /// just-reused pid is never mistaken for the process that was killed.
    pub fn escalate_now(&mut self, s: &mut Sampler) {
        s.refresh();
        let pending = std::mem::take(&mut self.pending);
        for p in &pending {
            let due = p.at + KILL_AFTER;
            while Instant::now() < due && s.alive(p.pid, p.start) {
                std::thread::sleep(Duration::from_millis(50));
            }
            self.finish(s, p);
        }
    }

    /// KILL a TERM survivor and leave a receipt.
    fn finish(&mut self, s: &Sampler, p: &Pending) {
        if s.alive(p.pid, p.start) {
            s.signal(p.pid, p.start, Signal::Kill);
            self.note(format!("escalated: pid {}", p.pid));
        }
    }

    /// One notification per batch of kills, not one per process.
    pub fn flush(&mut self) {
        if let Some(b) = banner(&self.batch) {
            notify(&b);
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
/// The pure half of a kill: which (pid, start) pairs the root takes down, and their total
/// RSS. Delivery (TERM now, KILL later) stays in `kill_tree`.
fn tree_targets(procs: &[Proc], root: &Row) -> (Vec<(u32, u64)>, u64) {
    let kids: HashMap<u32, Vec<&Proc>> = procs.iter().fold(HashMap::new(), |mut m, p| {
        m.entry(p.ppid).or_default().push(p);
        m
    });
    let mut stack = vec![(root.pid, root.start)];
    let mut targets = Vec::new();
    let mut total = 0u64;
    let mut seen = HashSet::new();
    while let Some((pid, start)) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        targets.push((pid, start));
        if let Some(children) = kids.get(&pid) {
            for c in children {
                total += c.rss;
                stack.push((c.pid, c.start));
            }
        }
    }
    (targets, total)
}

/// One banner per batch: the count when several land, the first receipt alone otherwise.
fn banner(batch: &[String]) -> Option<String> {
    match batch.len() {
        0 => None,
        1 => Some(batch[0].clone()),
        n => Some(format!("{n} processes killed · {}", batch[0])),
    }
}

#[cfg(all(target_os = "macos", not(test)))]
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
#[cfg(all(target_os = "linux", not(test)))]
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
#[cfg(all(target_os = "windows", not(test)))]
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
#[cfg(not(test))]
fn notify(msg: &str) {
    let _ = msg;
}

/// Tests never post banners.
#[cfg(test)]
fn notify(_msg: &str) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{self, GRACE};

    fn events_of(a: &Actor) -> Vec<String> {
        a.events.iter().cloned().collect()
    }

    fn tool(pid: u32, rss_mb: u64) -> Row {
        fixtures::row(pid, Kind::Tool, true, rss_mb, Some("orphan"))
    }

    fn target(pid: u32, rss_mb: u64) -> Row {
        fixtures::row(pid, Kind::Tool, false, rss_mb, Some("heavy agent child"))
    }

    fn run(a: &mut Actor, v: &View, m: &Mem, at: Instant) {
        let s = fixtures::sampler();
        a.tick(&s, &[], v, m, at);
    }

    #[test]
    fn note_is_ring_buffer_newest_first() {
        let mut a = fixtures::actor(true, false);
        for i in 0..(EVENTS + 10) {
            a.note(format!("e{i}"));
        }
        assert_eq!(a.events.len(), EVENTS);
        assert_eq!(a.events.front().unwrap(), &format!("e{}", EVENTS + 9));
        assert_eq!(a.events.back().unwrap(), "e10");
    }

    // regression: fresh orphans must not die on the tick they are first seen
    #[test]
    fn fresh_orphan_survives_first_tick() {
        let mut a = fixtures::actor(true, false);
        let v = fixtures::view(vec![tool(50, 900)]);
        run(&mut a, &v, &fixtures::mem(Pressure::Normal), Instant::now());
        assert!(!events_of(&a).iter().any(|e| e.contains("auto: orphan")));
        assert!(a.first_seen.contains_key(&(50, 1)));
    }

    #[test]
    fn orphan_tool_dies_after_grace() {
        let mut a = fixtures::actor(true, false);
        let v = fixtures::view(vec![tool(50, 900)]);
        let t0 = Instant::now();
        run(&mut a, &v, &fixtures::mem(Pressure::Normal), t0);
        run(
            &mut a,
            &v,
            &fixtures::mem(Pressure::Normal),
            t0 + GRACE + Duration::from_secs(1),
        );
        let ev = events_of(&a);
        assert!(
            ev.iter()
                .any(|e| e.contains("auto: orphan") && e.contains("p50")),
            "expected a kill receipt, got {ev:?}"
        );
        assert!(a.first_seen.is_empty(), "grace bookkeeping cleaned up");
    }

    // regression: plain node/python orphans must never auto-die unless --auto-dev
    #[test]
    fn orphan_runtime_spared_by_default_swept_with_auto_dev() {
        let runtime = || fixtures::view(vec![fixtures::row(60, Kind::Dev, true, 200, None)]);

        let mut a = fixtures::actor(true, false);
        let t0 = Instant::now();
        run(&mut a, &runtime(), &fixtures::mem(Pressure::Normal), t0);
        run(
            &mut a,
            &runtime(),
            &fixtures::mem(Pressure::Normal),
            t0 + GRACE + Duration::from_secs(1),
        );
        assert!(!events_of(&a).iter().any(|e| e.contains("auto:")));

        let mut a = fixtures::actor(true, true);
        run(&mut a, &runtime(), &fixtures::mem(Pressure::Normal), t0);
        run(
            &mut a,
            &runtime(),
            &fixtures::mem(Pressure::Normal),
            t0 + GRACE + Duration::from_secs(1),
        );
        assert!(events_of(&a).iter().any(|e| e.contains("auto: orphan")));
    }

    #[test]
    fn no_auto_never_kills() {
        let mut a = fixtures::actor(false, true);
        let v = fixtures::view(vec![tool(50, 900)]);
        let t0 = Instant::now();
        run(&mut a, &v, &fixtures::mem(Pressure::Normal), t0);
        run(
            &mut a,
            &v,
            &fixtures::mem(Pressure::Normal),
            t0 + GRACE + Duration::from_secs(1),
        );
        assert!(!events_of(&a).iter().any(|e| e.contains("auto:")));
    }

    // regression: the killer must re-alert on every pressure transition, not spam or stay silent
    #[test]
    fn pressure_change_alerts_once_per_transition() {
        let mut a = fixtures::actor(false, false);
        run(
            &mut a,
            &fixtures::view(vec![]),
            &fixtures::mem(Pressure::Warn),
            Instant::now(),
        );
        run(
            &mut a,
            &fixtures::view(vec![]),
            &fixtures::mem(Pressure::Warn),
            Instant::now(),
        );
        run(
            &mut a,
            &fixtures::view(vec![]),
            &fixtures::mem(Pressure::Critical),
            Instant::now(),
        );
        let warns = events_of(&a)
            .iter()
            .filter(|e| e.contains("pressure"))
            .count();
        assert_eq!(warns, 2, "warn then critical, no repeats");
    }

    // regression: critical pressure kills the heaviest rule-A target at most once per cooldown
    #[test]
    fn critical_kills_heaviest_target_with_cooldown() {
        let mut a = fixtures::actor(true, false);
        let v = fixtures::view(vec![target(50, 100), target(60, 900)]);
        let m = fixtures::mem(Pressure::Critical);
        let t0 = Instant::now();
        run(&mut a, &v, &m, t0);
        let first = events_of(&a)
            .iter()
            .find(|e| e.contains("auto: critical pressure"))
            .cloned();
        assert!(
            first.as_deref().unwrap_or("").contains("p60"),
            "heaviest first: {first:?}"
        );
        run(&mut a, &v, &m, t0 + Duration::from_secs(5));
        let count = events_of(&a)
            .iter()
            .filter(|e| e.contains("auto: critical pressure"))
            .count();
        assert_eq!(count, 1, "cooldown holds within 10s");
    }

    // regression: the receipt must name why, root, and the whole subtree — the walk must
    // include every descendant, not only the root.
    #[test]
    fn tree_targets_walks_the_whole_subtree() {
        let procs = vec![
            fixtures::proc(50, 1, "node", "node root.js", 300),
            fixtures::proc(51, 50, "node", "node child.js", 700),
            fixtures::proc(52, 51, "node", "node grandchild.js", 100),
            fixtures::proc(60, 1, "node", "node unrelated.js", 500),
        ];
        let root = tool(50, 300);
        let (targets, total) = tree_targets(&procs, &root);
        let pids: Vec<u32> = targets.iter().map(|(p, _)| *p).collect();
        assert_eq!(
            pids,
            vec![50, 51, 52],
            "root + descendants, unrelated excluded"
        );
        assert_eq!(
            total,
            800 << 20,
            "children only: root rss added by the caller"
        );
    }

    #[test]
    fn kill_tree_receipts_cover_descendants() {
        let mut a = fixtures::actor(true, false);
        let s = fixtures::sampler();
        let procs = vec![
            fixtures::proc(50, 1, "node", "node root.js", 300),
            fixtures::proc(51, 50, "node", "node child.js", 700),
        ];
        let r = tool(50, 300);
        a.kill_tree(&s, &procs, &r, "test: why");
        let ev = events_of(&a).last().unwrap().clone();
        assert!(ev.contains("test: why") && ev.contains("p50"));
        assert!(ev.contains("1000M"), "receipt totals the subtree: {ev}");
        // the pids are absent here, so no TERM could be armed — pending stays empty
        assert!(a.pending.is_empty());
        // and the batch is drained only by flush(): kill_tree leaves it staged
        assert_eq!(a.batch.len(), 1);
    }

    // regression: escalate_now must reap expired pending without waiting (old bug: quitting
    // dropped them) and must leave a visible receipt.
    #[test]
    fn escalate_now_sweeps_expired_pending() {
        let mut a = fixtures::actor(true, false);
        let mut s = fixtures::sampler();
        a.pending.push(Pending {
            pid: 3_999_999,
            start: 1,
            at: Instant::now() - KILL_AFTER,
        });
        a.escalate_now(&mut s);
        assert!(a.pending.is_empty());
        assert!(
            events_of(&a).is_empty(),
            "dead pids reap silently — no phantom receipts"
        );
    }

    #[test]
    fn flush_sends_one_banner_per_batch() {
        let mut a = fixtures::actor(true, false);
        assert_eq!(banner(&[]), None);
        assert_eq!(banner(&["kill one".into()]), Some("kill one".into()));
        assert_eq!(
            banner(&["kill one".into(), "kill two".into()]),
            Some("2 processes killed · kill one".into())
        );
        a.batch.push("kill one".into());
        a.flush();
        assert!(a.batch.is_empty(), "drained");
    }
}
