//! Shared test fixtures. `#[cfg(test)]`-only: costs the release binary nothing.

use crate::act::Actor;
use crate::app::App;
use crate::classify::{Group, Kind, Row, View};
use crate::sys::{Mem, Pressure, Proc, Sampler};
use crate::theme::Theme;
use std::time::Duration;

/// The grace period the test actors are built with.
pub const GRACE: Duration = Duration::from_secs(60);

/// A Row with the classify contract spelled out: the caller states `why` explicitly instead
/// of re-encoding the marking rule, so a classify change cannot silently drift the fixtures.
#[allow(clippy::too_many_arguments)]
pub fn row(pid: u32, kind: Kind, orphan: bool, rss_mb: u64, why: Option<&'static str>) -> Row {
    Row {
        pid,
        start: 1,
        depth: 0,
        label: format!("p{pid}"),
        rss: rss_mb << 20,
        cpu: 0.0,
        kind,
        orphan,
        subtree: rss_mb << 20,
        why,
        workers: 0,
    }
}

/// One group holding every row; nothing in the tests depends on the title's value.
pub fn view(rows: Vec<Row>) -> View {
    View {
        groups: vec![Group {
            title: "ORPHANS".into(),
            orphans: true,
            open: true,
            rows,
        }],
    }
}

pub fn proc(pid: u32, ppid: u32, name: &str, cmd: &str, rss_mb: u64) -> Proc {
    Proc {
        pid,
        ppid,
        name: name.into(),
        cmd: cmd.into(),
        rss: rss_mb << 20,
        cpu: 0.0,
        start: 1,
    }
}

/// No initial system walk: the fake pids never match the real table anyway.
pub fn sampler() -> Sampler {
    Sampler::cold()
}

/// No log file and no banners: receipts are asserted through `events` and `batch`.
pub fn actor(auto: bool, auto_dev: bool) -> Actor {
    Actor::cold(auto, GRACE, auto_dev)
}

pub fn mem(pressure: Pressure) -> Mem {
    Mem {
        used: 8 << 30,
        total: 16 << 30,
        swap_used: 1 << 30,
        swap_total: 4 << 30,
        pressure,
    }
}

/// An App literal for renderer and key tests; nothing samples a real system.
pub fn app(view: View, panic: Option<crate::app::Panic>) -> App {
    App {
        theme: Theme::detect(),
        sampler: Sampler::cold(),
        actor: Actor::cold(true, GRACE, false),
        procs: vec![],
        view,
        mem: Mem::default(),
        sel: 0,
        anchor: None,
        panic,
        rss_limit: 500 << 20,
        auto_dev: false,
        open_map: std::collections::HashMap::new(),
        quit: false,
    }
}
