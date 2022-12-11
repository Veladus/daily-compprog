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

    let opts_arc = Arc::new(opts);
    let opts_arc2 = opts_arc.clone();
    let opts_arc1 = opts_arc;

    let cf_client_arc = Arc::new(codeforces::Client::new());
    let cf_client_arc2 = cf_client_arc.clone();
    let cf_client_arc1 = cf_client_arc;

    let (sched_send, sched_recv) = mpsc::unbounded_channel();
    let (telegram_send, telegram_recv) = mpsc::unbounded_channel();

    // Initialize and run subsystems
    Toplevel::new()
        .start("scheduler", move |subsys| {
            scheduler::subsystem_handler(
                opts_arc1,
                sched_recv,
                telegram_send,
                cf_client_arc1,
                subsys,
            )
        })
        .start("telegram bot", move |subsys| {
            telegram_bot::subsystem_handler(
                opts_arc2,
                telegram_recv,
                sched_send,
                cf_client_arc2,
                subsys,
            )
        })
        .catch_signals()
        .handle_shutdown_requests(Duration::from_secs(20))
        .await
        .map_err(Into::into)
}
