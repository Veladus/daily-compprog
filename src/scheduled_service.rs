use std::collections::hash_map::Entry;
use std::collections::HashMap;
use teloxide::prelude::*;

use crate::options;
use crate::telegram_bot_service::{ChannelState, TelegramControlCommand};
use async_cron_scheduler::{Job, JobId, Scheduler};
use chrono::Local;
use miette::{IntoDiagnostic, Result};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_graceful_shutdown::SubsystemHandle;

pub const CRON_SCHEDULE: &str = "* * * * * * *";

use SchedulerControlCommand::*;
#[derive(Debug, Clone)]
pub enum SchedulerControlCommand {
    StartDailyMessages { chat_id: ChatId },
}

type SchedulerStorage = HashMap<ChatId, JobId>;
type MyScheduler = Scheduler<Local>;

async fn daily_message(
    chat_id: ChatId,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    log::info!("Daily message to {chat_id:?}");
    Ok(())
}

async fn start_daily_schedule(
    chat_id: ChatId,
    sched_storage_rw: Arc<RwLock<SchedulerStorage>>,
    scheduler_rw: Arc<RwLock<MyScheduler>>,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    log::info!("starting stuff for {chat_id}");
    let mut scheduler = scheduler_rw.as_ref().write().await;

    let job_id = {
        let job = Job::cron(CRON_SCHEDULE).into_diagnostic()?;
        scheduler.insert(job, move |_id| {
            tokio::spawn(daily_message(chat_id, telegram_send.clone()));
        })
    };

    if let Some(old_job_id) = sched_storage_rw
        .as_ref()
        .write()
        .await
        .insert(chat_id, job_id)
    {
        scheduler.remove(old_job_id);
    }

    Ok(())
}

async fn handle_control_command(
    command: SchedulerControlCommand,
    sched_storage_rw: Arc<RwLock<SchedulerStorage>>,
    scheduler_rw: Arc<RwLock<MyScheduler>>,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    match command {
        StartDailyMessages { chat_id } => {
            start_daily_schedule(chat_id, sched_storage_rw, scheduler_rw, telegram_send).await
        }
    }
}

pub async fn subsystem_handler(
    options: Arc<options::Options>,
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
        tokio::spawn(handle_control_command(
            command,
            storage_arc.clone(),
            scheduler_arc.clone(),
            telegram_send_arc.clone(),
        ))
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
    sched_recv.close();
    // process pending control commands
    while let Some(command) = sched_recv.recv().await {
        open_tasks.push(spawn_task(command));
    }

    log::debug!("{} open task(s) in scheduler service", open_tasks.len());
    for handle in open_tasks {
        handle.await.into_diagnostic()??;
    }

    // there are no more references to the scheduler, so when this function terminates the scheduler gets terminated as well
    log::info!("Shut down scheduler service");
    Ok(())
}
