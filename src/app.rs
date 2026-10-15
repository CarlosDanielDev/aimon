//! TUI state and keys. Sampling and killing live in `sys`/`act`; this file only routes.
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::act::Actor;
use crate::classify::{self, Kind, Row, View};
use crate::sys::{Mem, Proc, Sampler};
use crate::theme::Theme;

pub struct Panic {
    pub title: &'static str,
    /// The kill reason recorded in the log for everything confirmed here.
    pub why: &'static str,
    pub items: Vec<(Row, bool)>,
    pub sel: usize,
}

pub struct App {
    pub theme: Theme,
    pub sampler: Sampler,
    pub actor: Actor,
    pub procs: Vec<Proc>,
    pub view: View,
    pub mem: Mem,
    pub sel: usize,
    /// The row the cursor is pinned to. Groups re-sort every tick; the anchor keeps the cursor
    /// (and therefore `x`) on the same process instead of on the same offset.
    pub anchor: Option<(u32, u64)>,
    pub panic: Option<Panic>,
    pub rss_limit: u64,
    pub auto_dev: bool,
    pub quit: bool,
}

/// Re-pin the cursor after a tick: follow the anchored row, else clamp the old index.
/// regression: a re-sorted list used to move the kill target under the cursor.
fn repinned(view: &View, sel: usize, anchor: Option<(u32, u64)>) -> usize {
    let n = view.rows().count();
    if let Some(key) = anchor
        && let Some(i) = view.rows().position(|r| (r.pid, r.start) == key)
    {
        return i;
    }
    sel.min(n.saturating_sub(1))
}

/// One clamped cursor step, shared by the main list and the panic popup.
fn move_sel(sel: usize, len: usize, down: bool) -> usize {
    if down {
        (sel + 1).min(len.saturating_sub(1))
    } else {
        sel.saturating_sub(1)
    }
}

impl App {
    pub fn new(auto: bool, grace: Duration, rss_limit: u64, auto_dev: bool) -> Self {
        Self {
            theme: Theme::detect(),
            sampler: Sampler::new(),
            actor: Actor::new(auto, grace, auto_dev),
            procs: Vec::new(),
            view: View::default(),
            mem: Mem::default(),
            sel: 0,
            anchor: None,
            panic: None,
            rss_limit,
            auto_dev,
            quit: false,
        }
    }

    pub fn tick(&mut self) {
        let (procs, mem) = self.sampler.sample();
        self.view = classify::build(&procs, &self.sampler.tmux, self.rss_limit, self.auto_dev);
        self.actor
            .tick(&self.sampler, &procs, &self.view, &mem, Instant::now());
        self.procs = procs;
        self.mem = mem;
        let anchor = self.anchor.take();
        self.sel = repinned(&self.view, self.sel, anchor);
        self.anchor = anchor.filter(|_| {
            // the anchor only survives while its row is still on screen
            self.selected()
                .is_some_and(|r| Some((r.pid, r.start)) == anchor)
        });
    }

    pub fn selected(&self) -> Option<&Row> {
        self.view.rows().nth(self.sel)
    }

    fn step(&mut self, down: bool) {
        self.sel = move_sel(self.sel, self.view.rows().count(), down);
        self.anchor = self.selected().map(|r| (r.pid, r.start));
    }

    /// The only kill funnel. Protected rows are refused here, once, whatever the entry point.
    fn kill(&mut self, rows: Vec<Row>, why: &str) {
        for r in &rows {
            if r.kind == Kind::Protected {
                self.actor.note(format!("{} is protected", r.label));
            } else {
                self.actor.kill_tree(&self.sampler, &self.procs, r, why);
            }
        }
        self.actor.flush();
    }

    pub fn key(&mut self, k: KeyEvent) {
        if self.panic.is_some() {
            self.panic_key(k.code);
            return;
        }
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Down | KeyCode::Char('j') => self.step(true),
            KeyCode::Up | KeyCode::Char('k') => self.step(false),
            KeyCode::Char('t') => self.theme = self.theme.switched(),
            KeyCode::Char('a') => {
                self.actor.auto = !self.actor.auto;
                let on = if self.actor.auto { "on" } else { "off" };
                self.actor.note(format!("auto {on}"));
            }
            KeyCode::Char('x') => match self.selected().cloned() {
                Some(r) if r.kind == Kind::Protected => {
                    self.actor.note(format!("{} is protected", r.label))
                }
                Some(r) => self.open_panic(vec![r], "KILL", "manual"),
                None => {}
            },
            KeyCode::Char('X') => self.kill_group(),
            KeyCode::Char('p') => self.open_panic(self.view.targets(), "PANIC", "panic"),
            _ => {}
        }
    }

    /// Every dev process of the selected row's group, offered pre-marked.
    fn kill_group(&mut self) {
        let rows: Vec<Row> = self
            .view
            .locate(self.sel)
            .map(|(g, _)| g.rows.iter().filter(|r| r.kind.is_dev()).cloned().collect())
            .unwrap_or_default();
        if rows.is_empty() {
            self.actor.note("no dev processes in this group".into());
        } else {
            self.open_panic(rows, "KILL GROUP", "manual: group");
        }
    }

    /// No kill leaves this screen unconfirmed: rows arrive pre-marked, `enter` fires.
    fn open_panic(&mut self, rows: Vec<Row>, title: &'static str, why: &'static str) {
        if rows.is_empty() {
            self.actor.note("panic: nothing to kill".into());
        } else {
            self.panic = Some(Panic {
                title,
                why,
                items: rows.into_iter().map(|r| (r, true)).collect(),
                sel: 0,
            });
        }
    }

    fn panic_key(&mut self, code: KeyCode) {
        let Some(p) = self.panic.as_mut() else { return };
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.panic = None,
            KeyCode::Down | KeyCode::Char('j') => p.sel = move_sel(p.sel, p.items.len(), true),
            KeyCode::Up | KeyCode::Char('k') => p.sel = move_sel(p.sel, p.items.len(), false),
            KeyCode::Char(' ') => {
                if let Some(item) = p.items.get_mut(p.sel) {
                    item.1 = !item.1;
                }
            }
            KeyCode::Enter => {
                let rows = p
                    .items
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(r, _)| r.clone())
                    .collect();
                let why = p.why;
                self.panic = None;
                self.kill(rows, why);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Group;
    use ratatui::crossterm::event::KeyModifiers;

    fn row(pid: u32, kind: Kind, orphan: bool) -> Row {
        Row {
            pid,
            start: 1,
            depth: 0,
            label: format!("p{pid}"),
            rss: 100 << 20,
            cpu: 0.0,
            kind,
            orphan,
            subtree: 100 << 20,
            why: if orphan && kind == Kind::Tool {
                Some("orphan")
            } else {
                None
            },
        }
    }

    fn view(rows: Vec<Row>) -> View {
        View {
            groups: vec![Group {
                title: "g".into(),
                orphans: false,
                rows,
            }],
        }
    }

    fn app(view: View) -> App {
        App {
            theme: Theme::detect(),
            sampler: Sampler::new(),
            actor: Actor::new(true, Duration::from_secs(60), false),
            procs: vec![],
            view,
            mem: Mem::default(),
            sel: 0,
            anchor: None,
            panic: None,
            rss_limit: 500 << 20,
            auto_dev: false,
            quit: false,
        }
    }

    #[test]
    fn move_sel_clamps_both_ends() {
        assert_eq!(move_sel(0, 3, false), 0);
        assert_eq!(move_sel(2, 3, true), 2);
        assert_eq!(move_sel(0, 3, true), 1);
        assert_eq!(move_sel(2, 3, false), 1);
        assert_eq!(move_sel(0, 0, true), 0, "empty list stays put");
    }

    // regression: the cursor followed an index, so a re-sort moved the kill target
    #[test]
    fn repinned_follows_the_row_not_the_offset() {
        let v = view(vec![
            row(50, Kind::Tool, true),
            row(60, Kind::Dev, false),
            row(70, Kind::Dev, false),
        ]);
        // anchor on pid 60 (index 1); the list flips underneath
        let mut flipped = view(vec![
            row(70, Kind::Dev, false),
            row(60, Kind::Dev, false),
            row(50, Kind::Tool, true),
        ]);
        flipped.groups[0].rows.reverse();
        assert_eq!(repinned(&v, 1, Some((60, 1))), 1);
        // pid 60 is now at index 1 in the flipped order too — use a real swap to prove it
        let swapped = view(vec![
            row(60, Kind::Dev, false),
            row(50, Kind::Tool, true),
            row(70, Kind::Dev, false),
        ]);
        assert_eq!(
            repinned(&swapped, 1, Some((60, 1))),
            0,
            "follows the row to its new index"
        );
        // anchor gone: fall back to the clamped index
        assert_eq!(repinned(&v, 9, Some((99, 1))), 2);
        assert_eq!(repinned(&v, 2, None), 2);
    }

    fn key_app(mut a: App, code: KeyCode) -> App {
        a.key(KeyEvent::new(code, KeyModifiers::NONE));
        a
    }

    // regression: x used to fire instantly — it must confirm through the popup
    #[test]
    fn x_opens_confirm_popup_not_an_instant_kill() {
        let a = key_app(
            app(view(vec![row(50, Kind::Tool, true)])),
            KeyCode::Char('x'),
        );
        let p = a.panic.expect("modal opens");
        assert_eq!(p.items.len(), 1);
        assert!(p.items[0].1, "pre-marked");
        assert!(a.actor.events.is_empty(), "nothing died yet");
    }

    #[test]
    fn x_on_protected_refuses_with_a_note() {
        let a = key_app(
            app(view(vec![row(9, Kind::Protected, false)])),
            KeyCode::Char('x'),
        );
        assert!(a.panic.is_none());
        assert!(a.actor.events[0].contains("is protected"));
    }

    #[test]
    fn big_x_offers_the_group_dev_rows() {
        let rows = vec![
            row(50, Kind::Tool, true),
            row(9, Kind::Protected, false),
            row(60, Kind::Dev, false),
        ];
        let a = key_app(app(view(rows)), KeyCode::Char('X'));
        let p = a.panic.expect("modal opens");
        let pids: Vec<u32> = p.items.iter().map(|(r, _)| r.pid).collect();
        assert_eq!(pids, vec![50, 60], "dev rows only, protected skipped");
    }

    #[test]
    fn big_x_on_devless_group_notes_instead() {
        let a = key_app(
            app(view(vec![row(9, Kind::Protected, false)])),
            KeyCode::Char('X'),
        );
        assert!(a.panic.is_none());
        assert!(a.actor.events[0].contains("no dev processes"));
    }

    #[test]
    fn panic_enter_kills_marked_and_closes() {
        let mut a = key_app(
            app(view(vec![row(50, Kind::Tool, true)])),
            KeyCode::Char('x'),
        );
        a.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)); // unmark
        a.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)); // mark again
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(a.panic.is_none(), "modal closed");
        let ev = a
            .actor
            .events
            .iter()
            .find(|e| e.contains("manual"))
            .expect("kill receipt");
        assert!(ev.contains("p50"), "{ev}");
    }

    #[test]
    fn panic_space_toggles_and_esc_cancels() {
        let mut a = key_app(
            app(view(vec![row(50, Kind::Tool, true)])),
            KeyCode::Char('x'),
        );
        a.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.panic.is_none());
        assert!(a.actor.events.is_empty(), "cancel kills nothing");
    }

    #[test]
    fn a_toggles_auto_and_q_quits() {
        let mut a = app(view(vec![]));
        a.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!a.actor.auto, "auto starts on, first press turns it off");
        a.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(a.actor.auto);
        a.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(a.quit);
    }
}
