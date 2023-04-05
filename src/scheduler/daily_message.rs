use crate::codeforces;
use crate::options::Options;
use crate::scheduler::{util, MyScheduler, SchedulerStorage};
use crate::telegram_bot::TelegramControlCommand;
use crate::telegram_bot::TelegramControlCommand::SetAndNotifyDailyProblem;
use futures::StreamExt;
use miette::{IntoDiagnostic, Result};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use teloxide::prelude::*;
use tokio::sync::{mpsc, RwLock};
use xorshift::{Rng, SeedableRng, Xorshift128};

async fn daily_message(
    chat_id: ChatId,
    telegram_send: &mpsc::UnboundedSender<TelegramControlCommand>,
    cf_client: &codeforces::Client,
) -> Result<()> {
    let channel_state = util::get_channel_state(chat_id, telegram_send).await?;

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

    let known_problems: HashSet<codeforces::Problem> = {
        futures::stream::iter(channel_state.registered_users().values())
            .filter_map(|handle| async {
                match handle.get_submissions(cf_client).await {
                    Ok(submissions) => Some(submissions),
                    Err(err) => {
                        log::warn!("Error getting submissions for {}\n{}", handle.as_str(), err);
                        None
                    }
                }
            })
            .flat_map(|submissions| {
                futures::stream::iter(submissions.into_iter().map(|submission| submission.problem))
            })
            .collect()
            .await
    };

    log::info!("Starting to prepare daily message for {chat_id:?}");
    let problem = loop {
        let tag_index = (rng.next_u64() as usize) % codeforces::TAGS.len();
        let problems = cf_client
            .get_problems_by_tag(std::iter::once(codeforces::TAGS[tag_index]))
            .await?;
        let mut problems: Vec<_> = problems
            .into_iter()
            .filter(|problem| {
                problem.rating.map_or(false, |rating| {
                    channel_state.rating_range().contains(&rating)
                }) && !known_problems.contains(problem)
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
        .send(SetAndNotifyDailyProblem { chat_id, problem })
        .into_diagnostic()
}

pub(super) async fn start(
    options: Arc<Options>,
    chat_id: ChatId,
    sched_storage_rw: Arc<RwLock<SchedulerStorage>>,
    scheduler_rw: Arc<RwLock<MyScheduler>>,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
    cf_client: Arc<codeforces::Client>,
) -> Result<()> {
    log::info!("Registered daily messages for {chat_id}");
    let mut scheduler = scheduler_rw.as_ref().write().await;

    let job_id = util::register_to_schedule(&options.messages_cron, &mut scheduler, move |_id| {
        let telegram_send_clone = telegram_send.clone();
        let cf_client_clone = cf_client.clone();
        tokio::spawn(async move {
            daily_message(
                chat_id,
                telegram_send_clone.as_ref(),
                cf_client_clone.as_ref(),
            )
            .await
            .unwrap()
        });
    })
    .await?;

    if let Some(old_job_id) = sched_storage_rw
        .as_ref()
        .write()
        .await
        .daily_message_job_ids
        .insert(chat_id, job_id)
    {
        scheduler.remove(old_job_id);
    }

    Ok(())
}
