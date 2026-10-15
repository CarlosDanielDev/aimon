//! Pure part: turns a flat process list into groups (orphans, tmux sessions, agents outside tmux)
//! and marks the kill targets of rule A. No I/O here, so every rule is tested with fake trees.
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::sys::Proc;

/// Shell noise: hidden from the tree (children hoist up), since eight identical
/// fish→fish→claude chains bury the agents users actually hunt for.
const SHELLS: &[&str] = &["fish", "zsh", "bash", "sh", "login", "pwsh", "powershell"];
/// Never a target, whatever else matches.
const PROTECTED: &[&str] = &[
    "tmux", "fish", "zsh", "bash", "sh", "nvim", "vim", "iterm2", "login", "aimon",
];
/// Workers an AI session leaves behind; auto-killed once orphaned.
const TOOLS: &[&str] = &[
    "vitest",
    "jest",
    "jest-worker",
    "processchild",
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

impl Kind {
    /// Spawned on purpose by a dev session (test runners, build tools, runtimes).
    pub fn is_dev(self) -> bool {
        matches!(self, Kind::Tool | Kind::Dev)
    }
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
    /// RSS of the whole subtree rooted here: what killing this row takes down.
    pub subtree: u64,
    /// Rule A: why this row is pre-marked in the panic popup, if it is.
    pub why: Option<&'static str>,
    /// Descendants under this row (agent rows show "+N" for their worker pool).
    pub workers: u32,
}

impl Row {
    /// Rule A orphan half: an orphaned tool worker, auto-killed after the grace period.
    pub fn is_orphan_tool(&self) -> bool {
        self.orphan && self.kind == Kind::Tool
    }

    /// An orphaned runtime (node, python, …): a daemon candidate, killed only under --auto-dev.
    pub fn is_orphan_runtime(&self) -> bool {
        self.orphan && self.kind == Kind::Dev
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub title: String,
    pub orphans: bool,
    /// Collapsible: closed groups hide their rows from every consumer of `rows()`.
    pub open: bool,
    pub rows: Vec<Row>,
}

impl Group {
    /// Always the sum of the rows, so it can never drift from them.
    pub fn rss(&self) -> u64 {
        self.rows.iter().map(|r| r.rss).sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct View {
    pub groups: Vec<Group>,
}

impl View {
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.groups
            .iter()
            .filter(|g| g.open)
            .flat_map(|g| g.rows.iter())
    }

    pub fn targets(&self) -> Vec<Row> {
        self.rows().filter(|r| r.why.is_some()).cloned().collect()
    }

    /// The group holding flat row `i` (the order `rows()` yields), and the row's index in it.
    /// Closed groups hold no visible rows and are skipped, matching `rows()`.
    pub fn locate(&self, i: usize) -> Option<(&Group, usize)> {
        let mut seen = 0;
        for g in &self.groups {
            if !g.open {
                continue;
            }
            if seen + g.rows.len() > i {
                return Some((g, i - seen));
            }
            seen += g.rows.len();
        }
        None
    }

    /// Screen lines printed above flat row `i`: one header per open group plus the rows
    /// before it, matching the renderer and `rows()`.
    pub fn line_of(&self, i: usize) -> usize {
        let (mut lines, mut seen) = (0, 0);
        for g in &self.groups {
            if !g.open {
                continue;
            }
            if seen + g.rows.len() > i {
                return lines + 1 + (i - seen);
            }
            lines += 1 + g.rows.len();
            seen += g.rows.len();
        }
        lines
    }
}

/// Terminal coding agents. Many run as `node …/bin/gemini` or `python …/aider`, so the first
/// args are matched by basename too, and versioned installs (`…/claude/versions/2.1.294`) by
/// their directory segment — a binary whose basename is the version number.
const AGENTS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "aider",
    "opencode",
    "cursor-agent",
    "amp",
    "goose",
    "crush",
    "qwen",
    "copilot",
    "droid",
];

/// The agent this process is, if any.
pub fn agent(name: &str, cmd: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    AGENTS.iter().copied().find(|ag| {
        name == *ag
            || cmd.split(' ').take(3).any(|a| {
                let b = last_seg(a);
                b == *ag || b.strip_suffix(".js") == Some(*ag) || agent_dir(ag, a)
            })
    })
}

fn last_seg(a: &str) -> &str {
    a.rsplit('/').next().unwrap_or(a)
}

/// A path segment named `ag` leading into a version — `claude/versions/2.1.294`,
/// `codex/1.2.3` — how versioned agent installs look. A bare directory that merely shares an
/// agent's name (`proj/amp/server.js`, `claude/config.json`) is not an agent.
fn agent_dir(ag: &str, a: &str) -> bool {
    let version_like = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');
    let (mut prev, mut prev2) = ("", "");
    for s in a.split('/') {
        if (prev == ag && version_like(s)) || (prev == "versions" && version_like(s) && prev2 == ag)
        {
            return true;
        }
        (prev2, prev) = (prev, s);
    }
    false
}

/// Apps installed in /Applications or /System (Homebrew's Python.app is not an app).
fn is_gui(cmd: &str) -> bool {
    (cmd.starts_with("/applications/") || cmd.starts_with("/system/"))
        && cmd.contains(".app/contents/")
}

/// The tool from TOOLS this process runs, if any.
fn tool_match(name: &str, cmd: &str) -> Option<&'static str> {
    TOOLS
        .iter()
        .copied()
        .find(|t| name.contains(t) || cmd.contains(t))
}

/// Test-facing form of the classification; production resolves the agent once in `Tree::new`.
#[cfg(test)]
pub fn kind(name: &str, cmd: &str) -> Kind {
    kind_of(name, cmd, agent(name, cmd))
}

fn kind_of(name: &str, cmd: &str, agent: Option<&'static str>) -> Kind {
    let name = name.to_lowercase();
    if agent.is_some() || PROTECTED.contains(&name.as_str()) || is_gui(cmd) {
        Kind::Protected
    } else if tool_match(&name, cmd).is_some() {
        Kind::Tool
    } else if RUNTIMES.contains(&name.as_str()) || name.starts_with("python") {
        Kind::Dev
    } else {
        Kind::Other
    }
}

fn label(agent: Option<&'static str>, kind: Kind, p: &Proc) -> String {
    let name = agent.unwrap_or(p.name.as_str());
    let text = match (kind == Kind::Tool)
        .then(|| tool_match(&p.name, &p.cmd))
        .flatten()
    {
        Some(t) if !name.contains(t) => format!("{name} ({t})"),
        _ => name.to_string(),
    };
    plain(&text)
}

/// Rendered-text invariant for anything derived from process argv or tmux names (both are
/// attacker-controllable on a shared machine): control characters and invisible format
/// characters (bidi overrides, zero-width, BOM) become spaces — they would otherwise forge
/// log lines, corrupt ratatui cells, or spoof rendered output. Applied once at the display
/// funnel (`label` and group titles), never at classification input: stripping there would
/// change matching semantics and could make a hostile process look Protected.
pub fn plain(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '\u{200B}'..='\u{200F}'
                        | '\u{202A}'..='\u{202E}'
                        | '\u{2060}'..='\u{2064}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{FEFF}'
                )
            {
                ' '
            } else {
                c
            }
        })
        .collect()
}

struct Tree<'a> {
    procs: &'a [Proc],
    auto_dev: bool,
    kinds: Vec<Kind>,
    agents: Vec<Option<&'static str>>,
    children: HashMap<u32, Vec<usize>>,
    total: Vec<u64>,
    done: Vec<bool>,
    desc: Vec<u32>,
    desc_done: Vec<bool>,
    seen: HashSet<usize>,
    rss_limit: u64,
}

impl<'a> Tree<'a> {
    fn new(procs: &'a [Proc], rss_limit: u64, auto_dev: bool) -> Self {
        let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, p) in procs.iter().enumerate() {
            children.entry(p.ppid).or_default().push(i);
        }
        // Agent identity resolved once per process; kind, label, walk and build all read it.
        let agents: Vec<Option<&'static str>> =
            procs.iter().map(|p| agent(&p.name, &p.cmd)).collect();
        let kinds: Vec<Kind> = procs
            .iter()
            .zip(&agents)
            .map(|(p, a)| kind_of(&p.name, &p.cmd, *a))
            .collect();
        let mut t = Self {
            procs,
            auto_dev,
            kinds,
            agents,
            children,
            total: vec![0; procs.len()],
            done: vec![false; procs.len()],
            desc: vec![0; procs.len()],
            desc_done: vec![false; procs.len()],
            seen: HashSet::new(),
            rss_limit,
        };
        for i in 0..procs.len() {
            t.subtree_rss(i, 0);
            t.descendants(i, 0);
        }
        for list in t.children.values_mut() {
            list.sort_by(|a, b| t.total[*b].cmp(&t.total[*a]));
        }
        t
    }

    /// Number of descendants under node `i` (worker-pool size for agent rows).
    fn descendants(&mut self, i: usize, guard: usize) -> u32 {
        if self.desc_done[i] || guard > 64 {
            return self.desc[i];
        }
        let pid = self.procs[i].pid;
        let mut n = 0u32;
        for k in self.children.get(&pid).cloned().unwrap_or_default() {
            if k != i {
                n += 1 + self.descendants(k, guard + 1);
            }
        }
        self.desc[i] = n;
        self.desc_done[i] = true;
        n
    }

    fn subtree_rss(&mut self, i: usize, guard: usize) -> u64 {
        if self.done[i] || guard > 64 {
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
        self.done[i] = true;
        self.total[i]
    }

    /// Depth-first rows under `i`. `orphan_root` and `under_agent` decide rule A.
    fn walk(
        &mut self,
        i: usize,
        depth: usize,
        under_agent: bool,
        orphan_root: bool,
        out: &mut Vec<Row>,
    ) {
        if !self.seen.insert(i) || depth > 32 {
            return;
        }
        let p = &self.procs[i];
        let kind = self.kinds[i];
        let over = under_agent && p.rss > self.rss_limit;
        let is_shell = self.agents[i].is_none() && SHELLS.contains(&p.name.to_lowercase().as_str());
        if !is_shell {
            let mut row = Row {
                pid: p.pid,
                start: p.start,
                depth,
                label: label(self.agents[i], kind, p),
                rss: p.rss,
                cpu: p.cpu,
                kind,
                orphan: orphan_root,
                subtree: self.total[i],
                why: None,
                workers: self.desc[i],
            };
            // Orphaned runtimes may be daemons on purpose: only pre-marked when --auto-dev opted in.
            row.why = if row.is_orphan_tool() || (self.auto_dev && row.is_orphan_runtime()) {
                Some("orphan")
            } else if kind.is_dev() && over {
                Some("heavy agent child")
            } else {
                None
            };
            out.push(row);
        }
        let agent_here = under_agent || self.agents[i].is_some();
        let child_depth = if is_shell { depth } else { depth + 1 };
        for k in self.children.get(&p.pid).cloned().unwrap_or_default() {
            self.walk(k, child_depth, agent_here, false, out);
        }
    }
}

fn group(title: String, orphans: bool, rows: Vec<Row>) -> Option<Group> {
    (!rows.is_empty()).then_some(Group {
        title,
        orphans,
        open: true,
        rows,
    })
}

/// `tmux`: pane pid -> session name.
pub fn build(procs: &[Proc], tmux: &HashMap<u32, String>, rss_limit: u64, auto_dev: bool) -> View {
    let mut t = Tree::new(procs, rss_limit, auto_dev);
    let index: HashMap<u32, usize> = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    let mut groups = Vec::new();

    let mut rows = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        if p.ppid == 1 && t.kinds[i].is_dev() {
            t.walk(i, 0, false, true, &mut rows);
        }
    }
    groups.extend(group("ORPHANS".into(), true, rows));

    // Session-name keyed and sorted, so group order never flips when pane order drifts.
    let mut sessions: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (pane, name) in tmux {
        if let Some(&i) = index.get(pane) {
            sessions.entry(name.as_str()).or_default().push(i);
        }
    }
    let mut tail = Vec::new();
    for (name, panes) in sessions {
        let mut rows = Vec::new();
        for i in panes {
            t.walk(i, 0, false, false, &mut rows);
        }
        tail.extend(group(format!("tmux:{}", plain(name)), false, rows));
    }

    let mut rows = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        let parent_agent = index.get(&p.ppid).is_some_and(|j| t.agents[*j].is_some());
        if t.agents[i].is_some() && !parent_agent && !t.seen.contains(&i) {
            t.walk(i, 0, false, false, &mut rows);
        }
    }
    tail.extend(group("outside tmux".into(), false, rows));
    tail.sort_by_key(|g| std::cmp::Reverse(g.rss()));
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
        assert_eq!(
            kind("node", "node /opt/homebrew/bin/gemini"),
            Kind::Protected
        );
        assert_eq!(
            kind("python3.12", "python3.12 /u/.local/bin/aider --model x"),
            Kind::Protected
        );
        assert_eq!(kind("codex", "codex"), Kind::Protected);
        assert_eq!(kind("node", "node /x/amplify.js"), Kind::Dev);
        assert_eq!(kind("mds", "/usr/libexec/mds"), Kind::Other);
        assert_eq!(
            kind(
                "Python",
                "/opt/homebrew/python.app/contents/macos/python x.py"
            ),
            Kind::Dev
        );
    }

    // regression: jest worker pools escaped the tool sweep (plain `node processChild.js`)
    #[test]
    fn jest_worker_pools_are_tools() {
        assert_eq!(
            kind("node", "node /x/jest-worker/processChild.js"),
            Kind::Tool
        );
        assert_eq!(kind("processChild", "processChild"), Kind::Tool);
    }

    // regression: shell chains buried the agents — shells are hidden and their children
    // hoist up one depth level per skipped shell.
    #[test]
    fn shell_chains_collapse_onto_the_agent() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB, false);
        let tm = v
            .groups
            .iter()
            .find(|g| g.title == "tmux:projects")
            .expect("tmux group");
        assert!(
            tm.rows.iter().all(|r| r.label != "fish"),
            "shells are not rendered"
        );
        let claude = tm
            .rows
            .iter()
            .find(|r| r.label == "claude")
            .expect("agent shown");
        assert_eq!(
            claude.depth, 0,
            "hoisted: two shells skipped, no depth drift"
        );
        assert_eq!(claude.workers, 2, "direct children counted");
    }

    #[test]
    fn closed_groups_hide_rows_from_every_consumer() {
        let (procs, tmux) = sample();
        let mut v = build(&procs, &tmux, 500 * MB, false);
        let full = v.rows().count();
        v.groups[0].open = false;
        assert_eq!(v.rows().count(), full - v.groups[0].rows.len());
        assert!(
            v.locate(0).is_none()
                || v.groups[0].rows.is_empty()
                || v.locate(0).unwrap().0.title != v.groups[0].title
        );
    }

    // security: rendered text derived from argv must carry no invisible or control characters
    #[test]
    fn plain_neutralizes_control_and_invisible_characters() {
        assert_eq!(plain("claude"), "claude");
        assert_eq!(plain("hide\nFAKE"), "hide FAKE");
        assert_eq!(plain("col\tA"), "col A");
        assert_eq!(
            plain("a\u{202E}cod\u{200B}"),
            "a cod ",
            "bidi + zero-width become spaces"
        );
        assert_eq!(plain("\u{FEFF}bom"), "bom");
    }

    #[test]
    fn labels_from_hostile_argv_arrive_clean() {
        let p = Proc {
            pid: 1,
            ppid: 0,
            name: "evil\u{202E}".into(),
            cmd: "evil\u{202E} --x".into(),
            rss: 0,
            cpu: 0.0,
            start: 0,
        };
        assert!(!label(None, Kind::Other, &p).contains('\u{202E}'));
    }

    #[test]
    fn agent_match_is_case_insensitive_on_name() {
        assert_eq!(kind("Claude", "claude --model x"), Kind::Protected);
    }

    #[test]
    fn empty_view_locates_nothing() {
        let v = View::default();
        assert!(v.locate(0).is_none());
        assert_eq!(v.line_of(0), 0);
    }

    #[test]
    fn agent_paths() {
        // Versioned install: matched by its directory segment, at any argv position.
        assert_eq!(
            agent("2.1.294", "/u/.local/share/claude/versions/2.1.294"),
            Some("claude")
        );
        assert_eq!(
            agent(
                "node",
                "node --max-old-space-size=4096 /users/x/.local/share/claude/versions/2.1.294"
            ),
            Some("claude")
        );
        assert_eq!(
            agent("node", "node /opt/codex/1.2.3/bin/run.js"),
            Some("codex")
        );
        // A directory that merely shares an agent's name is not an agent.
        assert_eq!(agent("node", "node /home/me/proj/amp/server.js"), None);
        assert_eq!(agent("node", "node /x/claude/config.json"), None);
        // User data paths in later args never make a process an agent.
        assert_eq!(
            agent("node", "node /x/serve.js /users/c/.claude/x.json"),
            None
        );
        // A bare name still matches, and no agent anywhere matches nothing.
        assert_eq!(agent("codex", "codex"), Some("codex"));
        assert_eq!(agent("mds", "/usr/libexec/mds"), None);
    }

    #[test]
    fn orphans_first_and_marked() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB, false);
        let first = &v.groups[0];
        assert!(first.orphans);
        assert_eq!(first.rss(), 1500 * MB);
        let pids: Vec<u32> = first.rows.iter().map(|r| r.pid).collect();
        assert_eq!(pids, vec![50, 51, 60]);
        assert!(first.rows[0].orphan);
        assert_eq!(first.rows[0].why, Some("orphan"));
        assert_eq!(first.rows[0].subtree, 1300 * MB, "subtree covers the child");
        assert!(
            first.rows[2].why.is_none(),
            "orphan dev server is shown, not pre-marked"
        );
        assert!(!first.rows[1].orphan, "child dies with its root");
    }

    #[test]
    fn heavy_claude_child_is_target_light_is_not() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB, false);
        let tm = v
            .groups
            .iter()
            .find(|g| g.title == "tmux:projects")
            .expect("tmux group");
        let t: Vec<u32> = tm
            .rows
            .iter()
            .filter(|r| r.why.is_some())
            .map(|r| r.pid)
            .collect();
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
        let v = build(&procs, &tmux, 0, false);
        assert!(
            v.rows()
                .filter(|r| r.kind == Kind::Protected)
                .all(|r| r.why.is_none())
        );
        assert!(v.rows().all(|r| r.pid != 70), "orphan nvim is not shown");
    }

    #[test]
    fn auto_dev_opts_orphan_runtimes_in() {
        let (procs, tmux) = sample();
        // default: an orphaned `node server.js` is shown but never pre-marked
        let safe = build(&procs, &tmux, 500 * MB, false);
        assert_eq!(safe.rows().find(|r| r.pid == 60).unwrap().why, None);
        // --auto-dev: it becomes a rule-A target
        let aggressive = build(&procs, &tmux, 500 * MB, true);
        assert_eq!(
            aggressive.rows().find(|r| r.pid == 60).unwrap().why,
            Some("orphan")
        );
    }

    #[test]
    fn claude_outside_tmux_grouped() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB, false);
        assert!(
            v.groups
                .iter()
                .any(|g| g.title == "outside tmux" && g.rows[0].pid == 80)
        );
    }

    #[test]
    fn flat_index_mapping() {
        let (procs, tmux) = sample();
        let v = build(&procs, &tmux, 500 * MB, false);
        // locate() and line_of() agree with the flat order rows() yields: every group costs
        // one header line, every row one line.
        let mut expect_line = 1;
        for (i, row) in v.rows().enumerate() {
            let (g, at) = v.locate(i).expect("in range");
            assert!(std::ptr::eq(&g.rows[at], row));
            assert_eq!(v.line_of(i), expect_line, "flat {i}");
            expect_line += if at + 1 == g.rows.len() { 2 } else { 1 };
        }
        assert!(v.locate(v.rows().count()).is_none());
    }
}
