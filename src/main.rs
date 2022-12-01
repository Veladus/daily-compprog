mod codeforces;
mod options;
mod scheduler;
mod telegram_bot;

use miette::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_graceful_shutdown::Toplevel;

#[tokio::main]
async fn main() -> Result<()> {
    // Query command line options and initialize logging
    let opts = options::parse();

    let opts_rc = Arc::new(opts);
    let opts_rc1 = opts_rc.clone();
    let opts_rc2 = opts_rc;

    let (sched_send, sched_recv) = mpsc::unbounded_channel();
    let (telegram_send, telegram_recv) = mpsc::unbounded_channel();

    // Initialize and run subsystems
    Toplevel::new()
        .start("scheduler", move |subsys| {
            scheduler::subsystem_handler(opts_rc1, sched_recv, telegram_send, subsys)
        })
        .start("telegram bot", move |subsys| {
            telegram_bot::subsystem_handler(opts_rc2, telegram_recv, sched_send, subsys)
        })
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(3000))
        .await
        .map_err(Into::into)
}
