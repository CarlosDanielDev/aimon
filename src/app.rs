//! TUI state and keys. Sampling and killing live in `sys`/`act`; this file only routes.
use std::time::Duration;

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
    pub quit: bool,
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
    pub fn new(auto: bool, grace: Duration, rss_limit: u64) -> Self {
        Self {
            theme: Theme::detect(),
            sampler: Sampler::new(),
            actor: Actor::new(auto, grace),
            procs: Vec::new(),
            view: View::default(),
            mem: Mem::default(),
            sel: 0,
            anchor: None,
            panic: None,
            rss_limit,
            quit: false,
        }
    }

    pub fn tick(&mut self) {
        let (procs, mem) = self.sampler.sample();
        self.view = classify::build(&procs, &self.sampler.tmux, self.rss_limit);
        self.actor.tick(&self.sampler, &procs, &self.view, &mem);
        self.procs = procs;
        self.mem = mem;
        let n = self.view.rows().count();
        if let Some(key) = self.anchor {
            match self.view.rows().position(|r| (r.pid, r.start) == key) {
                Some(i) => self.sel = i,
                None => self.anchor = None,
            }
        }
        self.sel = self.sel.min(n.saturating_sub(1));
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
