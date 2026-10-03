//! ANSI escape codes for the banner and the log formatter.

pub const BOLD_RED: &str = "\x1b[1;31m";
pub const BOLD_YELLOW: &str = "\x1b[1;33m";
pub const BOLD_CYAN: &str = "\x1b[1;36m";
pub const DIM_GREY: &str = "\x1b[2;90m";
pub const RESET: &str = "\x1b[0m";

pub const BOLD: &str = "\x1b[1m";
pub const BOLD_GREEN: &str = "\x1b[1;32m";
pub const BOLD_BLUE: &str = "\x1b[1;34m";
pub const BOLD_MAGENTA: &str = "\x1b[1;35m";
pub const BRIGHT_WHITE: &str = "\x1b[97m";
pub const GREY: &str = "\x1b[90m";

pub const BADGE_ERROR: &str = "\x1b[1;97;41m";
pub const BADGE_WARN: &str = "\x1b[1;30;43m";
pub const BADGE_INFO: &str = "\x1b[1;30;46m";
pub const BADGE_DEBUG: &str = "\x1b[1;30;47m";
pub const BADGE_TRACE: &str = "\x1b[30;100m";
