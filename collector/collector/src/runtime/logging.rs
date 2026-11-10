//! Log formatter: local time, a coloured level badge, the module, and key=value fields coloured by key.

use std::{
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter};

use crate::runtime::ansi::{
    BADGE_DEBUG, BADGE_ERROR, BADGE_INFO, BADGE_TRACE, BADGE_WARN, BOLD, BOLD_BLUE, BOLD_GREEN,
    BOLD_MAGENTA, BOLD_RED, BOLD_YELLOW, BRIGHT_WHITE, DIM_GREY, GREY, RESET,
};

// Consts for the log formatter.
const NO_COLOR_ENV: &str = "NO_COLOR";
const CRATE_PREFIX: &str = "croft::";

//
fn badge(level: Level) -> &'static str {
    match level {
        Level::Error => BADGE_ERROR,
        Level::Warn => BADGE_WARN,
        Level::Info => BADGE_INFO,
        Level::Debug => BADGE_DEBUG,
        Level::Trace => BADGE_TRACE,
    }
}

// value_color picks a value's colour from its key, so a workload label reads the same on every line.
fn value_color(key: &str, value: &str) -> &'static str {
    match (key, value) {
        ("workload", "gpu-onset") => BOLD_GREEN,
        ("workload", "gpu-held") => BOLD_YELLOW,
        ("workload", "no-gpu") => BOLD_BLUE,
        ("comm", _) => BOLD,
        ("container" | "tgid", _) => BOLD_MAGENTA,
        ("error" | "drops", _) => BOLD_RED,
        _ => BRIGHT_WHITE,
    }
}

fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

// paint colours each key=value word; other words keep the default colour.
fn paint(message: &str) -> String {
    message
        .split(' ')
        .map(|word| match word.split_once('=') {
            Some((key, value)) if is_key(key) => {
                format!(
                    "{GREY}{key}={RESET}{}{value}{RESET}",
                    value_color(key, value)
                )
            }
            _ => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn module(target: &str) -> &str {
    target.strip_prefix(CRATE_PREFIX).unwrap_or(target)
}

// local_time formats the wall clock as HH:MM:SS.mmm in the host's time zone.
fn local_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as libc::time_t;
    // SAFETY: an all-zero tm is valid; localtime_r writes only into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to live locals for the call's duration.
    unsafe { libc::localtime_r(&seconds, &mut tm) };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        now.subsec_millis()
    )
}

// init sets up the log formatter and initializes the logger.
pub fn init() {
    let color = std::env::var_os(NO_COLOR_ENV).is_none();
    // RUST_LOG overrides the info default.
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .format(move |buf, record| {
            let level = record.level();
            let target = module(record.target());
            let message = record.args().to_string();
            if !color {
                return writeln!(buf, "{} {level:<5} {target:<9} {message}", local_time());
            }
            writeln!(
                buf,
                "{DIM_GREY}{}{RESET} {} {level:<5} {RESET} {GREY}{target:<9}{RESET} {}",
                local_time(),
                badge(level),
                paint(&message)
            )
        })
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_coloured_and_words_are_kept() {
        let painted = paint("summary workload=gpu-onset rows=3 not a=field");
        assert!(painted.starts_with("summary "));
        assert!(painted.contains(&format!(
            "{GREY}workload={RESET}{BOLD_GREEN}gpu-onset{RESET}"
        )));
        assert!(painted.contains(&format!("{BRIGHT_WHITE}3{RESET}")));
        assert!(painted.contains(" not "));
    }

    #[test]
    fn words_with_odd_keys_stay_plain() {
        for word in ["=x", "Key=x", "a b", "path=/x=y"] {
            let painted = paint(word);
            assert_eq!(painted.contains(GREY), word == "path=/x=y", "{word}");
        }
    }

    #[test]
    fn module_drops_the_crate_prefix() {
        assert_eq!(module("croft::reporter"), "reporter");
        assert_eq!(module("aya::programs"), "aya::programs");
    }
}
