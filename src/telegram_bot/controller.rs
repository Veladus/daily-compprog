use miette::{IntoDiagnostic, Result};
use std::sync::Arc;
use teloxide::prelude::*;

use TelegramControlCommand::*;
#[derive(Debug, Clone)]
pub enum TelegramControlCommand {
    SendMessage { chat_id: ChatId, message: String },
}

pub async fn handle(command: TelegramControlCommand, bot: Arc<Bot>) -> Result<()> {
    match command {
        SendMessage { chat_id, message } => {
            bot.send_message(chat_id, message).await.into_diagnostic()?;
            Ok(())
        }
    }
}
