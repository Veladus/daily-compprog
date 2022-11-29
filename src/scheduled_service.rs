use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use teloxide::prelude::*;

use crate::telegram_bot_service::TelegramControlCommand;
use crate::{codeforces, options};
use async_cron_scheduler::{Job, JobId, Scheduler};
use chrono::Local;
use miette::{IntoDiagnostic, Result};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio_graceful_shutdown::SubsystemHandle;
use xorshift::{Rng, SeedableRng, Xorshift128};

pub const CRON_SCHEDULE: &str = "0 * * * * * *";

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
    let unix_time_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .into_diagnostic()?
        .as_secs();
    let chat_hash = {
        let mut hasher = DefaultHasher::new();
        chat_id.hash(&mut hasher);
        hasher.finish()
    };
    let states = [unix_time_s, chat_hash];
    let mut rng: Xorshift128 = SeedableRng::from_seed(&states[..]);

    log::info!("Starting to prepare daily message for {chat_id:?}");
    let client = codeforces::Client::new();
    let problem = loop {
        let tag_index = (rng.next_u64() as usize) % codeforces::TAGS.len();
        let problems = client
            .get_problems_by_tag(std::iter::once(codeforces::TAGS[tag_index]))
            .await?;
        let mut problems: Vec<_> = problems
            .into_iter()
            .filter(|problem| {
                if let Some(rating) = problem.rating {
                    (2000..=2400).contains(&rating)
                } else {
                    false
                }
            })
            .collect();
        log::debug!(
            "For tag {} there are {} admissible problems",
            codeforces::TAGS[tag_index],
            problems.len()
        );

        if !problems.is_empty() {
            break problems.swap_remove((rng.next_u64() as usize) % problems.len());
        }
        log::warn!("Tag {} has no viable problems", codeforces::TAGS[tag_index]);
    };

    log::info!("Sending daily message to {:?}", chat_id);
    telegram_send
        .as_ref()
        .send(TelegramControlCommand::SendMessage {
            chat_id,
            message: format!("Today's problem is: {}", problem.url()?),
        })
        .into_diagnostic()
}

async fn start_daily_schedule(
    chat_id: ChatId,
    sched_storage_rw: Arc<RwLock<SchedulerStorage>>,
    scheduler_rw: Arc<RwLock<MyScheduler>>,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    log::info!("Registered daily messages for {chat_id}");
    let mut scheduler = scheduler_rw.as_ref().write().await;

    let job_id = {
        let job = Job::cron(CRON_SCHEDULE).into_diagnostic()?;
        scheduler.insert(job, move |_id| {
            let telegram_send_clone = telegram_send.clone();
            tokio::spawn(async move { daily_message(chat_id, telegram_send_clone).await.unwrap() });
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
            handle_control_command(command, storage_clone, scheduler_clone, telegram_send_clone)
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
