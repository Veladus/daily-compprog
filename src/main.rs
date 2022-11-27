mod daily_messages;
mod options;

use std::sync::Arc;
use miette::Result;
use tokio::time::Duration;
use tokio_graceful_shutdown::Toplevel;

#[tokio::main]
async fn main() -> Result<()> {
    // Query command line options and initialize logging
    let opts = options::parse();
    let opts_rc = Arc::new(opts);

    // Initialize and run subsystems
    Toplevel::new()
        .start("daily messages", move |subsys| {
            daily_messages::daily_handler(opts_rc, subsys)
        })
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(1000))
        .await
        .map_err(Into::into)
}
