use miette::{miette, IntoDiagnostic, Result};
use std::sync::Arc;
use teloxide::dispatching::dialogue::Storage;
use teloxide::prelude::*;
use tokio::sync::oneshot;

use crate::telegram_bot::dispatcher::MyStorage;
use crate::telegram_bot::ChannelState;
use TelegramControlCommand::*;

#[derive(Debug)]
pub enum TelegramControlCommand {
    SendMessage {
        chat_id: ChatId,
        message: String,
    },
    GetChannelState {
        chat_id: ChatId,
        return_send: oneshot::Sender<ChannelState>,
    },
}

pub async fn handle(
    command: TelegramControlCommand,
    bot: Arc<Bot>,
    storage: Arc<MyStorage>,
) -> Result<()> {
    match command {
        SendMessage { chat_id, message } => {
            bot.send_message(chat_id, message).await.into_diagnostic()?;
            Ok(())
        }
        GetChannelState {
            chat_id,
            return_send,
        } => {
            let state: ChannelState = storage
                .clone()
                .get_dialogue(chat_id)
                .await
                .into_diagnostic()?
                .unwrap_or_default();
            let cloned_state = state.clone();
            storage
                .update_dialogue(chat_id, state)
                .await
                .into_diagnostic()?;

            return_send
                .send(cloned_state)
                .map_err(|_| miette!("Could not send channel state for {:?}", chat_id))
        }
    }
}
