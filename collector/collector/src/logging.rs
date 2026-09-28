//! Log formatter with one colour per level.

use std::io::Write;

use log::Level;

use crate::ansi::{BOLD_CYAN, BOLD_RED, BOLD_YELLOW, DIM_GREY, DIM_WHITE, RESET};

fn color(level: Level) -> &'static str {
    match level {
        Level::Error => BOLD_RED,
        Level::Warn => BOLD_YELLOW,
        Level::Info => BOLD_CYAN,
        Level::Debug => DIM_WHITE,
        Level::Trace => DIM_GREY,
    }
}

pub fn init() {
    env_logger::Builder::from_default_env()
        .format(|buf, record| {
            let level = record.level();
            writeln!(buf, "{}[{level:>5}]{RESET} {}", color(level), record.args())
        })
        .init();
}
