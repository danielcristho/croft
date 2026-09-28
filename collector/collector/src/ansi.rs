//! ANSI escape codes for the banner and the log formatter.

pub const BOLD_RED: &str = "\x1b[1;31m";
pub const BOLD_YELLOW: &str = "\x1b[1;33m";
pub const BOLD_CYAN: &str = "\x1b[1;36m";
pub const DIM_WHITE: &str = "\x1b[2;37m";
pub const DIM_GREY: &str = "\x1b[2;90m";
pub const RESET: &str = "\x1b[0m";
