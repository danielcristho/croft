//! Exit on SIGINT and SIGTERM, which a container's PID 1 otherwise ignores.

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use tokio::signal::unix::{SignalKind, signal};

use crate::runtime::ansi::LEAVE_SCREEN;

// Constants for the exit handler.
const EXIT_BASE: i32 = 128;
static ON_ALTERNATE_SCREEN: AtomicBool = AtomicBool::new(false);
static SAVED_TERMINAL: OnceLock<libc::termios> = OnceLock::new();

// The kernel applies no default action to PID 1, so without a handler Ctrl-C and `docker stop` do nothing.
pub fn exit_on_interrupt() {
    let handler = on_signal as extern "C" fn(libc::c_int);
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: the handler calls only tcsetattr, write and _exit, all async-signal-safe.
        unsafe { libc::signal(signal, handler as libc::sighandler_t) };
    }
}

// alternate_screen tells the handler whether an interrupt must restore the shell's screen first.
pub fn alternate_screen(active: bool) {
    ON_ALTERNATE_SCREEN.store(active, Ordering::SeqCst);
}

// saved_terminal gives the handler the settings an interrupt must put back on stdin.
pub fn saved_terminal(settings: libc::termios) {
    let _ = SAVED_TERMINAL.set(settings);
}

extern "C" fn on_signal(signal: libc::c_int) {
    if let Some(settings) = SAVED_TERMINAL.get() {
        // SAFETY: tcsetattr is async-signal-safe and the settings live for the whole process.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, settings) };
    }
    if ON_ALTERNATE_SCREEN.load(Ordering::SeqCst) {
        // SAFETY: write is async-signal-safe and the buffer is a static string.
        unsafe {
            libc::write(
                libc::STDOUT_FILENO,
                LEAVE_SCREEN.as_ptr().cast(),
                LEAVE_SCREEN.len(),
            )
        };
    }
    // SAFETY: _exit ends the process without running non-reentrant code.
    unsafe { libc::_exit(EXIT_BASE + signal) }
}

// shutdown waits for SIGINT or SIGTERM, and returns the name of the signal received.
pub async fn shutdown() -> std::io::Result<&'static str> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = terminate.recv() => Ok("SIGTERM"),
        _ = interrupt.recv() => Ok("SIGINT"),
    }
}
