use crate::codeforces;
use crate::scheduler::{MyScheduler, SchedulerStorage};
use crate::telegram_bot::TelegramControlCommand;
use async_cron_scheduler::Job;
use miette::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use teloxide::prelude::*;
use tokio::sync::{mpsc, oneshot, RwLock};
use xorshift::{Rng, SeedableRng, Xorshift128};

pub const CRON_SCHEDULE: &str = "0 30 7 * * * *";

async fn daily_message(
    chat_id: ChatId,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    let channel_state = {
        let (send, recv) = oneshot::channel();
        telegram_send
            .send(TelegramControlCommand::GetChannelState {
                chat_id,
                return_send: send,
            })
            .map_err(|_| miette!("Could not request channel state for {:?}", chat_id))?;
        recv.await.into_diagnostic()?
    };

    let mut rng: Xorshift128 = {
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
        SeedableRng::from_seed(&states[..])
    };

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
                problem.rating.map_or(false, |rating| {
                    channel_state.rating_range().contains(&rating)
                })
            })
            .collect();
        log::debug!(
            "For tag {} in chat {:?} there are {} admissible problems",
            codeforces::TAGS[tag_index],
            chat_id,
            problems.len()
        );

        if !problems.is_empty() {
            break problems.swap_remove((rng.next_u64() as usize) % problems.len());
        }
        log::warn!(
            "Tag {} has no viable problems in chat {:?}",
            codeforces::TAGS[tag_index],
            chat_id
        );
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

pub async fn start(
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
