// Integration e2e: runs the real binary. Spawning needs expect(), so relax it here only.
#![allow(clippy::expect_used)]

//! End-to-end checks against the real binary: flag surface, exit codes, help.

use std::process::Command;

fn aimon(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aimon"))
        .args(args)
        .output()
        .expect("binary runs")
}

#[test]
fn help_prints_usage_to_stdout_and_exits_zero() {
    let out = aimon(&["--help"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("usage: aimon"));
    assert!(s.contains("--auto-dev"), "new flags must show in help");
    assert!(s.contains("--rss-limit-mb"));
}

#[test]
fn unknown_option_exits_two_with_usage() {
    let out = aimon(&["--wat"]);
    assert_eq!(out.status.code(), Some(2));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("unknown option: --wat"));
    assert!(e.contains("usage: aimon"));
}

#[test]
fn missing_value_exits_two_naming_the_flag() {
    let out = aimon(&["--rss-limit-mb"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--rss-limit-mb needs a number"));
}

#[test]
fn help_short_flag_matches_long() {
    let out = aimon(&["-h"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("usage: aimon"));
}
