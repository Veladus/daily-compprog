mod daily_messages;
mod options;
mod telegram_bot;

use std::sync::Arc;
use miette::Result;
use tokio::time::Duration;
use tokio_graceful_shutdown::Toplevel;

#[tokio::main]
async fn main() -> Result<()> {
    // Query command line options and initialize logging
    let opts = options::parse();

    let opts_rc = Arc::new(opts);
    let opts_rc1 = opts_rc.clone();
    let opts_rc2 = opts_rc;

    // Initialize and run subsystems
    Toplevel::new()
        .start("daily messages", move |subsys| {
            daily_messages::daily_handler(opts_rc1, subsys)
        })
        .start("telegram bot", move |subsys| {
            telegram_bot::subsystem_handler(opts_rc2, subsys)
        })
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(3000))
        .await
        .map_err(Into::into)
}
