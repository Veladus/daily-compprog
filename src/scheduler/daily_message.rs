use crate::codeforces;
use crate::options::Options;
use crate::scheduler::{util, MyScheduler, SchedulerStorage};
use crate::telegram_bot::TelegramControlCommand;
use crate::telegram_bot::TelegramControlCommand::SetAndNotifyDailyProblem;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::{mpsc, RwLock};
use miette::{Result, IntoDiagnostic};

async fn daily_message(
    chat_id: ChatId,
    telegram_send: &mpsc::UnboundedSender<TelegramControlCommand>,
    cf_client: &codeforces::Client,
) -> Result<()> {
    let channel_state = crate::util::get_channel_state(chat_id, telegram_send).await?;

    log::info!("Starting to prepare daily message for {chat_id:?}");
    let problem = channel_state.find_daily_problem(cf_client, chat_id).await?;

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
