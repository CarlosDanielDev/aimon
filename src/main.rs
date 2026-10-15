//! aimon: watches what terminal coding agents spawn, alerts before the Mac runs out of memory, and kills
//! the leftovers (orphaned test/tool workers) before they drown the machine.
mod act;
mod app;
mod classify;
#[cfg(test)]
mod fixtures;
mod sys;
mod theme;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, DisableFocusChange, EnableFocusChange, Event, KeyEventKind};
use ratatui::crossterm::execute;

const USAGE: &str = "aimon — AI process monitor

usage: aimon [--daemon] [--no-auto] [--auto-dev] [--rss-limit-mb N] [--orphan-grace-s N] [--tick-ms N]

  --daemon           no TUI: auto-kill and alerts only
  --auto-dev         auto-kill orphaned runtimes (node, python) too — off by
                     default because they may be daemons you meant to keep
  --no-auto          never kills on its own
  --rss-limit-mb N   agent child above this becomes a panic target (500)
  --orphan-grace-s N orphaned worker is killed after N s (60)
  --tick-ms N        sampling interval (2000)";

#[derive(Debug)]
enum CliError {
    Help,
    Msg(String),
}

struct Opts {
    daemon: bool,
    auto: bool,
    auto_dev: bool,
    rss_limit: u64,
    grace: Duration,
    tick: Duration,
}

fn parse(args: &[String]) -> Result<Opts, CliError> {
    let mut o = Opts {
        daemon: false,
        auto: true,
        auto_dev: false,
        rss_limit: 500 << 20,
        grace: Duration::from_secs(60),
        tick: Duration::from_millis(2000),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut num = |name: &str| -> Result<u64, CliError> {
            it.next()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| CliError::Msg(format!("{name} needs a number")))
        };
        match a.as_str() {
            "--daemon" => o.daemon = true,
            "--no-auto" => o.auto = false,
            "--auto-dev" => o.auto_dev = true,
            // A 0 here would mean "every child is a target"; a huge N must not shift-overflow.
            "--rss-limit-mb" => o.rss_limit = num(a)?.max(1).saturating_mul(1 << 20),
            "--orphan-grace-s" => o.grace = Duration::from_secs(num(a)?.max(1)),
            "--tick-ms" => o.tick = Duration::from_millis(num(a)?.max(250)),
            "-h" | "--help" => return Err(CliError::Help),
            other => return Err(CliError::Msg(format!("unknown option: {other}\n\n{USAGE}"))),
        }
    }
    Ok(o)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse(&args) {
        Ok(o) => o,
        Err(CliError::Help) => {
            println!("{USAGE}");
            return;
        }
        Err(CliError::Msg(msg)) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    let result = if opts.daemon {
        daemon(&opts)
    } else {
        tui(&opts)
    };
    if let Err(e) = result {
        eprintln!("aimon: {e}");
        std::process::exit(1);
    }
}

fn daemon(o: &Opts) -> io::Result<()> {
    let mut app = app::App::new(o.auto, o.grace, o.rss_limit, o.auto_dev);
    loop {
        app.tick();
        while let Some(e) = app.actor.events.pop_back() {
            println!("{e}");
        }
        std::thread::sleep(o.tick);
    }
}

fn tui(o: &Opts) -> io::Result<()> {
    let mut app = app::App::new(o.auto, o.grace, o.rss_limit, o.auto_dev);
    let mut term = ratatui::init();
    let _ = execute!(io::stdout(), EnableFocusChange);
    let result = run(&mut term, &mut app, o.tick);
    let _ = execute!(io::stdout(), DisableFocusChange);
    ratatui::restore();
    // A TERM may not have landed yet and no next tick is coming: finish the job on every
    // exit path, error or not.
    app.actor.escalate_now(&mut app.sampler);
    result
}

fn run(term: &mut ratatui::DefaultTerminal, app: &mut app::App, tick: Duration) -> io::Result<()> {
    // ponytail: unfocused terminal samples 2.5x slower; nobody is looking, auto still runs.
    let mut every = tick;
    let mut next = Instant::now();
    while !app.quit {
        if Instant::now() >= next {
            app.tick();
            next = Instant::now() + every;
        }
        term.draw(|f| ui::draw(f, app))?;
        if event::poll(next.saturating_duration_since(Instant::now()))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => app.key(k),
                Event::FocusLost => every = tick * 5 / 2,
                Event::FocusGained => {
                    every = tick;
                    next = Instant::now();
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CliError, parse};

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn flags() {
        let o = parse(&args(
            "--daemon --no-auto --rss-limit-mb 800 --orphan-grace-s 5",
        ))
        .expect("ok");
        assert!(o.daemon && !o.auto);
        assert_eq!(o.rss_limit, 800 << 20);
        assert_eq!(o.grace.as_secs(), 5);
        assert!(parse(&args("--rss-limit-mb")).is_err());
        assert!(parse(&args("--wat")).is_err());
    }

    #[test]
    fn dangerous_zeros_are_floored() {
        let o = parse(&args("--rss-limit-mb 0 --orphan-grace-s 0")).expect("ok");
        assert_eq!(o.rss_limit, 1 << 20, "0 would target every child");
        assert_eq!(o.grace.as_secs(), 1, "0 would kill orphans instantly");
    }

    #[test]
    fn auto_dev_flag_parses() {
        let o = parse(&args("--auto-dev")).expect("ok");
        assert!(o.auto_dev);
        assert!(
            !parse(&args("--daemon")).expect("ok").auto_dev,
            "off by default"
        );
    }

    // regression: a huge --rss-limit-mb used to shift-overflow
    #[test]
    fn huge_rss_limit_saturates() {
        let o = parse(&args("--rss-limit-mb 99999999999999999")).expect("ok");
        assert_eq!(o.rss_limit, u64::MAX);
    }

    #[test]
    fn help_is_not_an_error() {
        assert!(matches!(parse(&args("--help")), Err(CliError::Help)));
    }
}
