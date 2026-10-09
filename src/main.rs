//! aimon: watches what AI sessions spawn, alerts before the Mac runs out of memory, and kills
//! the leftovers (orphaned test/tool workers) before they drown the machine.
mod act;
mod app;
mod classify;
mod sys;
mod theme;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, DisableFocusChange, EnableFocusChange, Event, KeyEventKind};
use ratatui::crossterm::execute;

const USAGE: &str = "aimon — monitor de processos da IA

uso: aimon [--daemon] [--no-auto] [--rss-limit-mb N] [--orphan-grace-s N] [--tick-ms N]

  --daemon           sem TUI: só auto-kill e alertas
  --no-auto          não mata nada sozinho
  --rss-limit-mb N   filho do claude acima disso vira alvo (500)
  --orphan-grace-s N worker órfão é morto após N s (60)
  --tick-ms N        intervalo de amostragem (2000)";

struct Opts {
    daemon: bool,
    auto: bool,
    rss_limit: u64,
    grace: Duration,
    tick: Duration,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        daemon: false,
        auto: true,
        rss_limit: 500 << 20,
        grace: Duration::from_secs(60),
        tick: Duration::from_millis(2000),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut num = |name: &str| -> Result<u64, String> {
            it.next()
                .and_then(|v| v.parse().ok())
                .ok_or(format!("{name} precisa de um número"))
        };
        match a.as_str() {
            "--daemon" => o.daemon = true,
            "--no-auto" => o.auto = false,
            "--rss-limit-mb" => o.rss_limit = num(a)? << 20,
            "--orphan-grace-s" => o.grace = Duration::from_secs(num(a)?),
            "--tick-ms" => o.tick = Duration::from_millis(num(a)?.max(250)),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("opção desconhecida: {other}\n\n{USAGE}")),
        }
    }
    Ok(o)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse(&args) {
        Ok(o) => o,
        Err(msg) => {
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
    let mut app = app::App::new(o.auto, o.grace, o.rss_limit);
    loop {
        app.tick();
        while let Some(e) = app.actor.events.pop_back() {
            println!("{e}");
        }
        std::thread::sleep(o.tick);
    }
}

fn tui(o: &Opts) -> io::Result<()> {
    let mut app = app::App::new(o.auto, o.grace, o.rss_limit);
    let mut term = ratatui::init();
    let _ = execute!(io::stdout(), EnableFocusChange);
    let result = run(&mut term, &mut app, o.tick);
    let _ = execute!(io::stdout(), DisableFocusChange);
    ratatui::restore();
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
    use super::parse;

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
}
