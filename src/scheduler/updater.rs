use crate::codeforces;
use crate::scheduler::{util, MyScheduler, SchedulerStorage};
use crate::telegram_bot::TelegramControlCommand;
use crate::telegram_bot::TelegramControlCommand::UpdateSolvingStatus;
use futures::stream::{self, StreamExt};
use miette::{miette, IntoDiagnostic, Result};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::ChatId;
use tokio::sync::{mpsc, RwLock};

const CRON_SCHEDULE: &str = "30 * * * * * *";

async fn update(
    chat_id: ChatId,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    let channel_state = util::get_channel_state(chat_id, telegram_send.as_ref()).await?;
    let current_problem = channel_state
        .current_daily_problem()
        .as_ref()
        .ok_or_else(|| miette!("Tried to update daily message without daily problem"))?;
    let client = codeforces::Client::new();

    let verdict_data: HashMap<codeforces::Handle, codeforces::VerdictCategory> =
        stream::iter(channel_state.registered_users().values())
            .filter_map(|handle| {
                let client_ref = &client;
                async move {
                    let submissions_res = client_ref.get_user_submissions(handle.as_str()).await;
                    match submissions_res {
                        Err(report) => {
                            log::error!(
                                "Error getting submissions for {}\n{}",
                                handle.as_str(),
                                report
                            );
                            None
                        }
                        Ok(submissions) => submissions
                            .iter()
                            .filter_map(|submission| {
                                submission
                                    .verdict
                                    .filter(|_| &submission.problem == current_problem)
                                    .map(|verdict| verdict.category())
                            })
                            .max()
                            .map(|category| (handle.clone(), category)),
                    }
                }
            })
            .collect()
            .await;

    telegram_send
        .send(UpdateSolvingStatus {
            chat_id,
            status: verdict_data,
        })
        .into_diagnostic()
}

pub(super) async fn start(
    chat_id: ChatId,
    sched_storage_rw: Arc<RwLock<SchedulerStorage>>,
    scheduler_rw: Arc<RwLock<MyScheduler>>,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    log::info!("Registered updater for {chat_id}");
    let mut scheduler = scheduler_rw.as_ref().write().await;

    let job_id = util::register_to_schedule(CRON_SCHEDULE, &mut scheduler, move |_id| {
        let telegram_send_clone = telegram_send.clone();
        tokio::spawn(async move { update(chat_id, telegram_send_clone).await.unwrap() });
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
