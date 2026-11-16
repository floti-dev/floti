//! `floti wait-host-exit`: what the desktop runs, from the floti it starts
//! hosts with, after it has asked this computer's host to restart for an
//! update. A host that takes `restart` stops answering on its socket first
//! and lets go of its data folder last, once its sessions are handed to the
//! keeper and its store is released, which can take seconds. Until then a
//! new host would only find the folders held and exit, so this waits until
//! no host holds the data folder's lock, then exits 0, and the desktop starts
//! the one new host after it.
//!
//! It resolves the host's folders from its environment, as `floti host`
//! does, connects to nothing and ends nothing. With no host running it exits
//! 0 at once. Past its deadline, or when the lock cannot be read, it says why
//! on standard error, as one line, and exits [`BridgeExit::Failed`]'s status.

use std::io::Write;

use floti_host::bridge::BridgeExit;
use floti_host::platform::folders::{self, UserEnv};

use crate::update_stop::host_exited;

/// Waits until no host holds this computer's host folders, and answers the
/// exit status.
pub fn run() -> u8 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => return failed(&format!("could not start: {err}")),
    };
    let waited = match folders::resolve(&UserEnv::from_process()) {
        Ok(folders) => runtime.block_on(host_exited(&folders.data)),
        Err(err) => Err(err.to_string()),
    };
    // A wait that ran out may still be blocked on the lock.
    runtime.shutdown_background();
    match waited {
        Ok(()) => 0,
        Err(why) => failed(&why),
    }
}

/// Says `why` on standard error, as one line, and answers the failure status.
fn failed(why: &str) -> u8 {
    let line = why.replace(['\r', '\n'], " ");
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "floti wait-host-exit: {line}").and_then(|()| err.flush());
    BridgeExit::Failed.code()
}
