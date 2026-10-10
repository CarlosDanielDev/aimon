//! Drawing only. Ratatui diffs the buffer, so an unchanged frame writes nothing to the terminal.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{App, Panic};
use crate::classify::{Group, Kind, Row};
use crate::sys::Pressure;
use crate::theme::{Theme, human};

const GB: u64 = 1 << 30;

pub fn draw(f: &mut Frame, app: &App) {
    let th = &app.theme;
    f.render_widget(Block::default().style(th.ground()), f.area());
    let [title, gauges, body, event, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let auto = if app.actor.auto {
        ("● auto", th.ok)
    } else {
        ("○ auto off", th.muted)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" aimon ", th.bold(th.brand)),
            Span::styled("· AI processes ", th.muted()),
            Span::styled(auto.0, th.bold(auto.1)),
        ])),
        title,
    );
    draw_gauges(f, app, gauges);
    draw_tree(f, app, body);
    let last = app.actor.events.front().map_or("", String::as_str);
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {last}"), th.fg(th.warn))),
        event,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            " ↑↓ navigate · x kill · X kill group · p panic · a auto · t theme · q quit",
            th.muted(),
        )),
        keys,
    );
    if let Some(p) = &app.panic {
        draw_panic(f, th, p);
    }
}

/// Scroll offset that keeps `sel` visible inside a `visible`-line window.
fn scroll_top(sel: usize, visible: usize) -> u16 {
    u16::try_from(sel.saturating_sub(visible.saturating_sub(1))).unwrap_or(u16::MAX)
}

fn bar(th: &Theme, used: u64, total: u64, width: u16) -> Line<'static> {
    let ratio = if total == 0 {
        0.0
    } else {
        used as f64 / total as f64
    };
    let color = match ratio {
        r if r >= 0.9 => th.bad,
        r if r >= 0.75 => th.warn,
        _ => th.ok,
    };
    let w = usize::from(width.saturating_sub(16));
    let fill = ((ratio * w as f64).round() as usize).min(w);
    let (on, off) = th.bar();
    Line::from(vec![
        Span::styled(on.to_string().repeat(fill), th.fg(color)),
        Span::styled(off.to_string().repeat(w - fill), th.muted()),
        Span::styled(
            format!(" {}/{}", human(used), human(total)),
            th.bold(th.value),
        ),
    ])
}

fn panel<'a>(th: &Theme, title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(th.border())
        .border_style(th.muted())
        .title(Span::styled(format!(" {title} "), th.bold(th.brand)))
}

fn draw_gauges(f: &mut Frame, app: &App, area: Rect) {
    let th = &app.theme;
    let m = &app.mem;
    let cols = Layout::horizontal([Constraint::Fill(1); 3]).split(area);
    let inner = cols[0].width.saturating_sub(2);
    for (i, (title, used, total)) in [
        ("RAM", m.used, m.total),
        ("swap", m.swap_used, m.swap_total),
    ]
    .into_iter()
    .enumerate()
    {
        f.render_widget(
            Paragraph::new(bar(th, used, total, inner)).block(panel(th, title)),
            cols[i],
        );
    }
    let (glyph, color) = match m.pressure {
        Pressure::Normal => ("●", th.ok),
        Pressure::Warn => ("▲", th.warn),
        Pressure::Critical => ("✖", th.bad),
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("{glyph} {}", m.pressure.label()),
            th.bold(color),
        ))
        .block(panel(th, "pressure")),
        cols[2],
    );
}

fn rss_style(th: &Theme, rss: u64) -> ratatui::style::Style {
    if rss >= GB {
        th.bold(th.bad)
    } else if rss >= GB / 2 {
        th.bold(th.warn)
    } else {
        th.fg(th.text)
    }
}

fn row_line(th: &Theme, r: &Row, width: usize, selected: bool) -> Line<'static> {
    let (glyph, color) = match (r.why.is_some(), r.orphan, r.kind) {
        (true, _, _) => ("✖", th.bad),
        (_, true, _) => ("○", th.warn),
        (_, _, Kind::Protected) => ("◆", th.accent),
        (_, _, k) if k.is_dev() => ("·", th.value),
        _ => ("·", th.muted),
    };
    let indent = "  ".repeat(r.depth.min(8));
    let name_w = width.saturating_sub(indent.len() + 28).max(8);
    let label: String = r.label.chars().take(name_w).collect();
    let mut line = Line::from(vec![
        Span::raw(format!(" {indent}")),
        Span::styled(format!("{glyph} "), th.fg(color)),
        Span::styled(format!("{label:<name_w$}"), th.fg(th.text)),
        Span::styled(format!("{:>7}", human(r.rss)), rss_style(th, r.rss)),
        Span::styled(format!(" {:>5.1}%", r.cpu), th.muted()),
        Span::styled(format!(" {:>7}", r.pid), th.muted()),
    ]);
    if selected {
        line = line.style(th.selected());
    }
    line
}

fn group_line(th: &Theme, g: &Group) -> Line<'static> {
    let color = if g.orphans { th.bad } else { th.accent };
    Line::from(vec![
        Span::styled(format!(" ▌{} ", g.title), th.bold(color)),
        Span::styled(
            format!("{} · {} procs", human(g.rss()), g.rows.len()),
            th.muted(),
        ),
    ])
}

fn draw_tree(f: &mut Frame, app: &App, area: Rect) {
    let th = &app.theme;
    let width = usize::from(area.width.saturating_sub(2));
    let h = usize::from(area.height.saturating_sub(2));
    let sel_line = app.view.line_of(app.sel);
    let top = scroll_top(sel_line, h) as usize;
    // Only the visible window is formatted; `at` counts lines whether or not they are drawn.
    let mut lines = Vec::with_capacity(h);
    let mut at = 0;
    let mut i = 0;
    for g in &app.view.groups {
        if at >= top && lines.len() < h {
            lines.push(group_line(th, g));
        }
        at += 1;
        for r in &g.rows {
            if at >= top && lines.len() < h {
                lines.push(row_line(th, r, width, i == app.sel));
            }
            at += 1;
            i += 1;
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled(" no AI processes in sight", th.muted()));
    }
    f.render_widget(
        Paragraph::new(lines)
            .scroll((top as u16, 0))
            .block(panel(th, "processes")),
        area,
    );
}

fn draw_panic(f: &mut Frame, th: &Theme, p: &Panic) {
    let a = f.area();
    let w = a.width.saturating_mul(3) / 4;
    let h = u16::try_from(p.items.len() + 4)
        .unwrap_or(u16::MAX)
        .min(a.height.saturating_sub(2));
    let area = Rect::new(a.x + (a.width - w) / 2, a.y + (a.height - h) / 2, w, h);
    let total: u64 = p
        .items
        .iter()
        .filter(|(_, on)| *on)
        .map(|(r, _)| r.rss)
        .sum();
    let mut lines: Vec<Line> = p
        .items
        .iter()
        .enumerate()
        .map(|(i, (r, on))| {
            let mark = if *on { "[✖]" } else { "[ ]" };
            let kids = r.subtree.saturating_sub(r.rss);
            let extra = if kids > 0 {
                format!(" · +{} subtree", human(kids))
            } else {
                String::new()
            };
            let line = Line::from(vec![
                Span::styled(
                    format!(" {mark} "),
                    th.bold(if *on { th.bad } else { th.muted }),
                ),
                Span::styled(format!("{:>7} ", human(r.rss)), rss_style(th, r.rss)),
                Span::styled(format!("{} ", r.label), th.fg(th.text)),
                Span::styled(
                    format!("pid {} · {}{extra}", r.pid, r.why.unwrap_or("picked")),
                    th.muted(),
                ),
            ]);
            if i == p.sel {
                line.style(th.selected())
            } else {
                line
            }
        })
        .collect();
    lines.push(Line::styled(
        format!(
            " space toggles · enter kills ({}) · esc cancels",
            human(total)
        ),
        th.fg(th.warn),
    ));
    let visible = usize::from(h.saturating_sub(3));
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines)
            .scroll((scroll_top(p.sel, visible), 0))
            .block(panel(th, p.title).border_style(th.fg(th.bad)))
            .style(th.ground()),
        area,
    );
}
