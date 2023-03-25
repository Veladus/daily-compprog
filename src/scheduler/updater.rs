use crate::codeforces;
use crate::scheduler::{util, MyScheduler, SchedulerStorage};
use crate::telegram_bot::TelegramControlCommand;
use crate::telegram_bot::TelegramControlCommand::UpdateSolvingStatus;
use futures::stream::{self, StreamExt};
use miette::{IntoDiagnostic, Result};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::ChatId;
use tokio::sync::{mpsc, RwLock};

const CRON_SCHEDULE: &str = "30 0/5 * * * * *";

async fn update(
    chat_id: ChatId,
    telegram_send: &mpsc::UnboundedSender<TelegramControlCommand>,
    cf_client: &codeforces::Client,
) -> Result<()> {
    let channel_state = util::get_channel_state(chat_id, telegram_send).await?;
    let current_problem = match channel_state.current_daily_problem().as_ref() {
        Some(problem) => problem,
        None => {
            log::debug!("Tried to update daily message without daily problem");
            return Ok(());
        }
    };

    let submissions_per_handle: HashMap<codeforces::Handle, Vec<codeforces::Submission>> =
        stream::iter(channel_state.registered_users().values())
            .filter_map(|handle| async move {
                match handle.get_submissions(cf_client).await {
                    Ok(submissions) => Some((handle.clone(), submissions)),
                    Err(report) => {
                        log::error!(
                            "Error getting submissions for {}\n{}",
                            handle.as_str(),
                            report
                        );
                        None
                    }
                }
            })
            .collect()
            .await;

    let status_per_problem = {
        let mut status_per_problem: HashMap<
            codeforces::Problem,
            HashMap<codeforces::Handle, codeforces::VerdictCategory>,
        > = HashMap::new();

        for (handle, submissions) in submissions_per_handle {
            for submission in submissions {
                if let Some(verdict) = submission.verdict {
                    status_per_problem
                        .entry(submission.problem)
                        .or_insert_with(Default::default)
                        .entry(handle.clone())
                        .and_modify(|previous_category| {
                            *previous_category = Ord::max(*previous_category, verdict.category());
                        })
                        .or_insert_with(|| verdict.category());
                }
            }
        }

        status_per_problem
    };

    telegram_send
        .send(UpdateSolvingStatus {
            chat_id,
            status: status_per_problem,
        })
        .into_diagnostic()
}

pub(super) async fn start(
    chat_id: ChatId,
    sched_storage_rw: Arc<RwLock<SchedulerStorage>>,
    scheduler_rw: Arc<RwLock<MyScheduler>>,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
    cf_client: Arc<codeforces::Client>,
) -> Result<()> {
    log::info!("Registered updater for {chat_id}");
    let mut scheduler = scheduler_rw.as_ref().write().await;

    let job_id = util::register_to_schedule(CRON_SCHEDULE, &mut scheduler, move |_id| {
        let telegram_send_clone = telegram_send.clone();
        let cf_client_clone = cf_client.clone();
        tokio::spawn(async move {
            update(
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
        .update_message_job_ids
        .insert(chat_id, job_id)
    {
        scheduler.remove(old_job_id);
    }

    Ok(())
}
