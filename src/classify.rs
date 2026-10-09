//! Pure part: turns a flat process list into groups (orphans, tmux sessions, claude outside tmux)
//! and marks the kill targets of rule A. No I/O here, so every rule is tested with fake trees.
use std::collections::{HashMap, HashSet};

use crate::sys::Proc;

/// Never a target, whatever else matches.
const PROTECTED: &[&str] = &[
    "claude", "tmux", "fish", "zsh", "bash", "sh", "nvim", "vim", "iterm2", "login", "aimon",
];
/// Workers an AI session leaves behind; auto-killed once orphaned.
const TOOLS: &[&str] = &[
    "vitest",
    "jest",
    "esbuild",
    "tsserver",
    "playwright",
    "mcp",
    "pytest",
];
/// Runtimes that may be a worker or a dev server on purpose; never auto-killed when orphaned.
const RUNTIMES: &[&str] = &["node", "bun", "deno", "npm", "npx", "pnpm", "yarn", "tsx"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Protected,
    Tool,
    Dev,
    Other,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub pid: u32,
    pub start: u64,
    pub depth: usize,
    pub label: String,
    pub rss: u64,
    pub cpu: f32,
    pub kind: Kind,
    /// Root of an orphaned dev tree (parent is launchd).
    pub orphan: bool,
    /// Rule A: pre-marked in the panic popup.
    pub target: bool,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub title: String,
    pub orphans: bool,
    pub rss: u64,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Default)]
pub struct View {
    pub groups: Vec<Group>,
}

impl View {
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.groups.iter().flat_map(|g| g.rows.iter())
    }

    pub fn targets(&self) -> Vec<Row> {
        self.rows().filter(|r| r.target).cloned().collect()
    }
}

fn is_claude(name: &str, cmd: &str) -> bool {
    name == "claude" || cmd.starts_with("claude") || cmd.contains("/claude/versions/")
}

/// Apps installed in /Applications or /System (Homebrew's Python.app is not an app).
fn is_gui(cmd: &str) -> bool {
    (cmd.starts_with("/applications/") || cmd.starts_with("/system/"))
        && cmd.contains(".app/contents/")
}

pub fn kind(name: &str, cmd: &str) -> Kind {
    let name = name.to_lowercase();
    if is_claude(&name, cmd) || PROTECTED.contains(&name.as_str()) || is_gui(cmd) {
        Kind::Protected
    } else if TOOLS.iter().any(|t| name.contains(t) || cmd.contains(t)) {
        Kind::Tool
    } else if RUNTIMES.contains(&name.as_str()) || name.starts_with("python") {
        Kind::Dev
    } else {
        Kind::Other
    }
}

fn label(p: &Proc, kind: Kind) -> String {
    let name = if is_claude(&p.name.to_lowercase(), &p.cmd) {
        "claude"
    } else {
        p.name.as_str()
    };
    match TOOLS.iter().find(|t| p.cmd.contains(*t)) {
        Some(t) if kind == Kind::Tool && !name.contains(t) => format!("{name} ({t})"),
        _ => name.to_string(),
    }
}

struct Tree<'a> {
    procs: &'a [Proc],
    kinds: Vec<Kind>,
    children: HashMap<u32, Vec<usize>>,
    total: Vec<u64>,
    seen: HashSet<usize>,
    rss_limit: u64,
}

impl<'a> Tree<'a> {
    fn new(procs: &'a [Proc], rss_limit: u64) -> Self {
        let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, p) in procs.iter().enumerate() {
            children.entry(p.ppid).or_default().push(i);
        }
        let mut t = Self {
            procs,
            kinds: procs.iter().map(|p| kind(&p.name, &p.cmd)).collect(),
            children,
            total: vec![0; procs.len()],
            seen: HashSet::new(),
            rss_limit,
        };
        for i in 0..procs.len() {
            t.subtree_rss(i, 0);
        }
        for list in t.children.values_mut() {
            list.sort_by(|a, b| t.total[*b].cmp(&t.total[*a]));
        }
        t
    }

    fn subtree_rss(&mut self, i: usize, guard: usize) -> u64 {
        if self.total[i] > 0 || guard > 64 {
            return self.total[i];
        }
        let pid = self.procs[i].pid;
        let kids = self.children.get(&pid).cloned().unwrap_or_default();
        let sum = kids
            .iter()
            .filter(|k| **k != i)
            .map(|k| self.subtree_rss(*k, guard + 1))
            .sum::<u64>();
        self.total[i] = self.procs[i].rss + sum;
        self.total[i]
    }

    /// Depth-first rows under `i`. `orphan_root` and `under_claude` decide rule A.
    fn walk(
        &mut self,
        i: usize,
        depth: usize,
        under_claude: bool,
        orphan_root: bool,
        out: &mut Vec<Row>,
    ) {
        if !self.seen.insert(i) || depth > 32 {
            return;
        }
        let p = &self.procs[i];
        let kind = self.kinds[i];
        let dev = matches!(kind, Kind::Tool | Kind::Dev);
        let over = under_claude && p.rss > self.rss_limit;
        out.push(Row {
            pid: p.pid,
            start: p.start,
            depth,
            label: label(p, kind),
            rss: p.rss,
            cpu: p.cpu,
            kind,
            orphan: orphan_root,
            // An orphaned runtime (python, node) may be a daemon on purpose: shown, never pre-marked.
            target: (orphan_root && kind == Kind::Tool) || (dev && over),
        });
        let claude_here = under_claude || is_claude(&p.name.to_lowercase(), &p.cmd);
        for k in self.children.get(&p.pid).cloned().unwrap_or_default() {
            self.walk(k, depth + 1, claude_here, false, out);
        }
    }
}

fn group(title: String, orphans: bool, rows: Vec<Row>) -> Option<Group> {
    (!rows.is_empty()).then(|| Group {
        title,
        orphans,
        rss: rows.iter().map(|r| r.rss).sum(),
        rows,
    })
}

/// `tmux`: pane pid -> session name.
pub fn build(procs: &[Proc], tmux: &HashMap<u32, String>, rss_limit: u64) -> View {
    let mut t = Tree::new(procs, rss_limit);
    let index: HashMap<u32, usize> = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    let mut groups = Vec::new();

    let mut rows = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        if p.ppid == 1 && matches!(t.kinds[i], Kind::Tool | Kind::Dev) {
            t.walk(i, 0, false, true, &mut rows);
        }
    }
    groups.extend(group("ÓRFÃOS".into(), true, rows));

    let mut sessions: Vec<(&String, Vec<usize>)> = Vec::new();
    for (pane, name) in tmux {
        let Some(&i) = index.get(pane) else { continue };
        match sessions.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(i),
            None => sessions.push((name, vec![i])),
        }
    }
    let mut tail = Vec::new();
    for (name, panes) in sessions {
        let mut rows = Vec::new();
        for i in panes {
            t.walk(i, 0, false, false, &mut rows);
        }
        tail.extend(group(format!("tmux:{name}"), false, rows));
    }

    let mut rows = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        let parent_claude = index
            .get(&p.ppid)
            .is_some_and(|j| is_claude(&procs[*j].name.to_lowercase(), &procs[*j].cmd));
        if is_claude(&p.name.to_lowercase(), &p.cmd) && !parent_claude && !t.seen.contains(&i) {
            t.walk(i, 0, false, false, &mut rows);
        }
    }
    tail.extend(group("fora do tmux".into(), false, rows));
    tail.sort_by_key(|g| std::cmp::Reverse(g.rss));
    groups.extend(tail);
    View { groups }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1 << 20;

    fn p(pid: u32, ppid: u32, name: &str, cmd: &str, rss_mb: u64) -> Proc {
        Proc {
            pid,
            ppid,
            name: name.into(),
            cmd: cmd.into(),
            rss: rss_mb * MB,
            cpu: 0.0,
            start: 1,
        }
    }

    fn sample() -> (Vec<Proc>, HashMap<u32, String>) {
        let procs = vec![
            p(10, 1, "tmux", "tmux", 5),
            p(20, 10, "fish", "-fish", 5),
            p(
                30,
                20,
                "2.1.294",
                "/users/c/.local/share/claude/versions/2.1.294",
                300,
            ),
            p(
                40,
                30,
                "node",
                "node /x/node_modules/vitest/dist/forks.js",
                900,
            ),
            p(41, 30, "node", "node /x/mcp-server.js", 80),
            p(50, 1, "node", "node (vitest 1)", 1200),
            p(51, 50, "node", "node (vitest 1) child", 100),
            p(60, 1, "node", "node server.js", 200),
            p(70, 1, "nvim", "nvim", 50),
            p(80, 1, "claude", "claude", 300),
        ];
        let tmux = HashMap::from([(20, "projects".to_string())]);
        (procs, tmux)
    }

    #[test]
    fn kinds() {
        assert_eq!(
            kind("2.1.294", "/u/.local/share/claude/versions/2.1.294"),
            Kind::Protected
        );
        assert_eq!(kind("node", "node (vitest 1)"), Kind::Tool);
        assert_eq!(kind("node", "node server.js"), Kind::Dev);
        assert_eq!(
            kind("Code Helper", "/applications/x.app/contents/tsserver"),
            Kind::Protected
        );
        assert_eq!(kind("nvim", "nvim"), Kind::Protected);
        assert_eq!(kind("mds", "/usr/libexec/mds"), Kind::Other);
        assert_eq!(
            kind(
                "Python",
                "/opt/homebrew/python.app/contents/macos/python x.py"
            ),
            Kind::Dev
        );
    }

    #[test]
    fn orphans_first_and_marked() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB);
        let first = &v.groups[0];
        assert!(first.orphans);
        let pids: Vec<u32> = first.rows.iter().map(|r| r.pid).collect();
        assert_eq!(pids, vec![50, 51, 60]);
        assert!(first.rows[0].target && first.rows[0].orphan);
        assert!(
            !first.rows[2].target,
            "orphan dev server is shown, not pre-marked"
        );
        assert!(!first.rows[1].target, "child dies with its root");
    }

    #[test]
    fn heavy_claude_child_is_target_light_is_not() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB);
        let tm = v
            .groups
            .iter()
            .find(|g| g.title == "tmux:projects")
            .expect("tmux group");
        let t: Vec<u32> = tm.rows.iter().filter(|r| r.target).map(|r| r.pid).collect();
        assert_eq!(t, vec![40]);
        assert_eq!(
            tm.rows
                .iter()
                .find(|r| r.pid == 30)
                .map(|r| r.label.as_str()),
            Some("claude")
        );
    }

    #[test]
    fn protected_never_target() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 0);
        assert!(
            v.rows()
                .filter(|r| r.kind == Kind::Protected)
                .all(|r| !r.target)
        );
        assert!(v.rows().all(|r| r.pid != 70), "orphan nvim is not shown");
    }

    #[test]
    fn claude_outside_tmux_grouped() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB);
        assert!(
            v.groups
                .iter()
                .any(|g| g.title == "fora do tmux" && g.rows[0].pid == 80)
        );
    }
}
