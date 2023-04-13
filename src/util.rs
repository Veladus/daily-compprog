use teloxide::types::ChatId;
use tokio::sync::oneshot;

use crate::telegram_bot::{TelegramControlCommand, ChannelState};
use miette::{Result, miette, IntoDiagnostic};


pub async fn get_channel_state(
    chat_id: ChatId,
    telegram_send: &tokio::sync::mpsc::UnboundedSender<TelegramControlCommand>,
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
