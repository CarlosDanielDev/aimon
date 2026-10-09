//! TUI state and keys. Sampling and killing live in `sys`/`act`; this file only routes.
use std::time::Duration;

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::act::Actor;
use crate::classify::{self, Kind, Row, View};
use crate::sys::{Mem, Proc, Sampler};
use crate::theme::Theme;

pub struct Panic {
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
    pub panic: Option<Panic>,
    pub rss_limit: u64,
    pub quit: bool,
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
        self.sel = self.sel.min(self.view.rows().count().saturating_sub(1));
    }

    pub fn selected(&self) -> Option<&Row> {
        self.view.rows().nth(self.sel)
    }

    fn kill(&mut self, rows: Vec<Row>, why: &str) {
        for r in &rows {
            self.actor.kill_tree(&self.sampler, &self.procs, r, why);
        }
        self.actor.flush();
    }

    pub fn key(&mut self, k: KeyEvent) {
        if self.panic.is_some() {
            self.panic_key(k.code);
            return;
        }
        let n = self.view.rows().count();
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Down | KeyCode::Char('j') => {
                self.sel = (self.sel + 1).min(n.saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => self.sel = self.sel.saturating_sub(1),
            KeyCode::Char('t') => self.theme = self.theme.switched(),
            KeyCode::Char('a') => {
                self.actor.auto = !self.actor.auto;
                let on = if self.actor.auto {
                    "ligado"
                } else {
                    "desligado"
                };
                self.actor.note(format!("auto {on}"));
            }
            KeyCode::Char('x') => match self.selected().cloned() {
                Some(r) if r.kind == Kind::Protected => {
                    self.actor.note(format!("{} é protegido", r.label))
                }
                Some(r) => self.kill(vec![r], "manual"),
                None => {}
            },
            KeyCode::Char('X') => self.kill_group(),
            KeyCode::Char('p') => self.open_panic(),
            _ => {}
        }
    }

    /// Every dev process of the selected row's group; protected and unknown ones stay.
    fn kill_group(&mut self) {
        let mut seen = 0;
        let group = self.view.groups.iter().find(|g| {
            seen += g.rows.len();
            seen > self.sel
        });
        let rows: Vec<Row> = group
            .map(|g| {
                g.rows
                    .iter()
                    .filter(|r| matches!(r.kind, Kind::Tool | Kind::Dev))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if rows.is_empty() {
            self.actor.note("nada de dev nesse grupo".into());
        } else {
            self.kill(rows, "manual: grupo");
        }
    }

    fn open_panic(&mut self) {
        let items: Vec<(Row, bool)> = self.view.targets().into_iter().map(|r| (r, true)).collect();
        if items.is_empty() {
            self.actor.note("panic: nada pra matar".into());
        } else {
            self.panic = Some(Panic { items, sel: 0 });
        }
    }

    fn panic_key(&mut self, code: KeyCode) {
        let Some(p) = self.panic.as_mut() else { return };
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.panic = None,
            KeyCode::Down | KeyCode::Char('j') => {
                p.sel = (p.sel + 1).min(p.items.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => p.sel = p.sel.saturating_sub(1),
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
                self.panic = None;
                self.kill(rows, "panic");
            }
            _ => {}
        }
    }
}
