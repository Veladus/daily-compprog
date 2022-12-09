use crate::scheduler::{daily_message, updater, MyScheduler, SchedulerStorage};
use crate::telegram_bot::TelegramControlCommand;
use miette::*;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::{mpsc, RwLock};
use SchedulerControlCommand::*;

#[derive(Debug, Clone)]
pub enum SchedulerControlCommand {
    StartDailyMessages { chat_id: ChatId },
}

pub(super) async fn handle(
    command: SchedulerControlCommand,
    sched_storage_rw: Arc<RwLock<SchedulerStorage>>,
    scheduler_rw: Arc<RwLock<MyScheduler>>,
    telegram_send: Arc<mpsc::UnboundedSender<TelegramControlCommand>>,
) -> Result<()> {
    match command {
        StartDailyMessages { chat_id } => {
            daily_message::start(
                chat_id,
                sched_storage_rw.clone(),
                scheduler_rw.clone(),
                telegram_send.clone(),
            )
            .await?;
            updater::start(chat_id, sched_storage_rw, scheduler_rw, telegram_send).await
        }
    }
}
