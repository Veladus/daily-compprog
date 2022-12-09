use crate::scheduler::MyScheduler;
use crate::telegram_bot::{ChannelState, TelegramControlCommand};
use async_cron_scheduler::{Job, JobId};
use miette::{miette, IntoDiagnostic, Result};
use teloxide::types::ChatId;
use tokio::sync::{mpsc, oneshot};

pub(super) async fn register_to_schedule(
    cron_str: &str,
    scheduler: &mut MyScheduler,
    command: impl Fn(JobId) + Send + Sync + 'static,
) -> Result<JobId> {
    let job = Job::cron(cron_str).into_diagnostic()?;
    Ok(scheduler.insert(job, command))
}

pub(super) async fn get_channel_state(
    chat_id: ChatId,
    telegram_send: &mpsc::UnboundedSender<TelegramControlCommand>,
) -> Result<ChannelState> {
    let (send, recv) = oneshot::channel();
    telegram_send
        .send(TelegramControlCommand::GetChannelState {
            chat_id,
            return_send: send,
        })
        .map_err(|_| miette!("Could not request channel state for {:?}", chat_id))?;
    recv.await.into_diagnostic()
}
