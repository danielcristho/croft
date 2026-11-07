//! ANSI escape codes for the banner and the log formatter.

pub const BOLD_RED: &str = "\x1b[1;31m";
pub const BOLD_YELLOW: &str = "\x1b[1;33m";
pub const BOLD_CYAN: &str = "\x1b[1;36m";
pub const DIM_GREY: &str = "\x1b[2;90m";
pub const RESET: &str = "\x1b[0m";

// Wrapping stays off so a long row is clipped instead of pushing the header away; 1007 turns the mouse wheel into arrow keys.
pub const ENTER_SCREEN: &str = "\x1b[?1049h\x1b[2J\x1b[?25l\x1b[?7l\x1b[?1007h";
pub const LEAVE_SCREEN: &str = "\x1b[?1007l\x1b[?7h\x1b[?25h\x1b[?1049l";
pub const HOME: &str = "\x1b[H";
pub const CLEAR_LINE_END: &str = "\x1b[K";
pub const CLEAR_BELOW: &str = "\x1b[J";

pub const BOLD: &str = "\x1b[1m";
pub const BOLD_GREEN: &str = "\x1b[1;32m";
pub const BOLD_BLUE: &str = "\x1b[1;34m";
pub const BOLD_MAGENTA: &str = "\x1b[1;35m";
pub const BRIGHT_WHITE: &str = "\x1b[97m";
pub const GREY: &str = "\x1b[90m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";

pub const BAR_TITLE: &str = "\x1b[1;30;46m";
pub const BAR_HEADER: &str = "\x1b[1;30;42m";

pub const BADGE_ERROR: &str = "\x1b[1;97;41m";
pub const BADGE_WARN: &str = "\x1b[1;30;43m";
pub const BADGE_INFO: &str = "\x1b[1;30;46m";
pub const BADGE_DEBUG: &str = "\x1b[1;30;47m";
pub const BADGE_TRACE: &str = "\x1b[30;100m";
