//! Exit on SIGINT and SIGTERM, which a container's PID 1 otherwise ignores.

use std::sync::atomic::{AtomicBool, Ordering};

use tokio::signal::unix::{SignalKind, signal};

use crate::ansi::LEAVE_SCREEN;

// Shell convention: 128 plus the signal number.
const EXIT_BASE: i32 = 128;

static ON_ALTERNATE_SCREEN: AtomicBool = AtomicBool::new(false);

// The kernel applies no default action to PID 1, so without a handler Ctrl-C and `docker stop` do nothing.
pub fn exit_on_interrupt() {
    let handler = on_signal as extern "C" fn(libc::c_int);
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: the handler calls only write and _exit, both async-signal-safe.
        unsafe { libc::signal(signal, handler as libc::sighandler_t) };
    }
}

// alternate_screen tells the handler whether an interrupt must restore the shell's screen first.
pub fn alternate_screen(active: bool) {
    ON_ALTERNATE_SCREEN.store(active, Ordering::SeqCst);
}

extern "C" fn on_signal(signal: libc::c_int) {
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
