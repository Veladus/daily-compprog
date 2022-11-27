use std::sync::Arc;
use async_cron_scheduler::{Job, Scheduler};
use chrono::Local;
use log::info;
use miette::{IntoDiagnostic, Result};
use tokio_graceful_shutdown::SubsystemHandle;
use crate::options;

pub async fn daily_handler(options: Arc<options::Options>, subsys: SubsystemHandle) -> Result<()> {
    log::info!("Setting up daily message handler...");

    // setup schedule
    let (mut scheduler, sched_service) = Scheduler::<Local>::launch(tokio::time::sleep);

    let message_job = Job::cron(&options.messages_cron).into_diagnostic()?;
    let _message_job_id = scheduler.insert(message_job, |_id| {
        info!("daily message");
    });

    tokio::spawn(sched_service);
    log::info!("Set up of daily message handler finished");

    subsys.on_shutdown_requested().await;
    log::info!("Shutting down daily message handler");
    Ok(())
}
