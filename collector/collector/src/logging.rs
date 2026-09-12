// Logging initialization for the collector binary.

use std::io::Write;

use log::Level;

// init colors each log level
pub fn init() {
    env_logger::Builder::from_default_env()
        .format(|buf, record| {
            let color = match record.level() {
                Level::Error => "\x1b[1;31m",
                Level::Warn => "\x1b[1;33m",
                Level::Info => "\x1b[1;36m",
                Level::Debug => "\x1b[2;37m",
                Level::Trace => "\x1b[2;90m",
            };
            writeln!(
                buf,
                "{color}[{:>5}]\x1b[0m {}",
                record.level(),
                record.args()
            )
        })
        .init();
}
