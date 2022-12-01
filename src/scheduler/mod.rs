use crate::options;
use crate::telegram_bot::TelegramControlCommand;
use async_cron_scheduler::{JobId, Scheduler};
use chrono::Local;
use miette::{IntoDiagnostic, Result};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::{mpsc, RwLock};
use tokio_graceful_shutdown::SubsystemHandle;

mod controller;
mod daily_message;

pub use controller::SchedulerControlCommand;

type SchedulerStorage = HashMap<ChatId, JobId>;
type MyScheduler = Scheduler<Local>;

pub async fn subsystem_handler(
    _options: Arc<options::Options>,
    mut sched_recv: mpsc::UnboundedReceiver<SchedulerControlCommand>,
    telegram_send: mpsc::UnboundedSender<TelegramControlCommand>,
    subsys: SubsystemHandle,
) -> Result<()> {
    log::info!("Setting up scheduler service...");

    // setup schedule
    let (scheduler, sched_service) = MyScheduler::launch(tokio::time::sleep);
    let scheduler_arc = Arc::new(RwLock::new(scheduler));
    tokio::spawn(sched_service);

    // make data sharable
    let telegram_send_arc = Arc::new(telegram_send);
    let storage_arc = Arc::new(RwLock::new(SchedulerStorage::new()));

    log::info!("Set up scheduler service");

    let mut open_tasks = Vec::new();
    let spawn_task = |command| {
        let (storage_clone, scheduler_clone, telegram_send_clone) = (
            storage_arc.clone(),
            scheduler_arc.clone(),
            telegram_send_arc.clone(),
        );
        tokio::spawn(async move {
            controller::handle(command, storage_clone, scheduler_clone, telegram_send_clone)
                .await
                .unwrap()
        })
    };
    // main control loop
    loop {
        tokio::select! {
            _ = subsys.on_shutdown_requested() => break,
            command_opt = sched_recv.recv() => match command_opt {
                Some(command) => open_tasks.push(spawn_task(command)),
                None => subsys.on_shutdown_requested().await,
            },
        }

        // clean open_tasks to prevent memory leakage
        open_tasks.retain(|handle| !handle.is_finished());
    }

    log::info!("Shutting down scheduler service...");

    // process pending control commands
    sched_recv.close();
    while let Some(command) = sched_recv.recv().await {
        open_tasks.push(spawn_task(command));
    }

    log::debug!("{} open task(s) in scheduler service", open_tasks.len());
    for handle in open_tasks {
        handle.await.into_diagnostic()?;
    }

    // there are no more references to the scheduler, so when this function terminates the scheduler gets terminated as well
    log::info!("Shut down scheduler service");
    Ok(())
}
