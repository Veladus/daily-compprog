use miette::{miette, IntoDiagnostic, Result};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::dispatching::dialogue::Storage;
use teloxide::prelude::*;
use tokio::sync::oneshot;

use crate::codeforces;
use crate::telegram_bot::dispatcher::MyStorage;
use crate::telegram_bot::ChannelState;
use TelegramControlCommand::*;

#[derive(Debug)]
pub enum TelegramControlCommand {
    GetChannelState {
        chat_id: ChatId,
        return_send: oneshot::Sender<ChannelState>,
    },
    SetAndNotifyDailyProblem {
        chat_id: ChatId,
        problem: codeforces::Problem,
    },
    UpdateSolvingStatus {
        chat_id: ChatId,
        status:
            HashMap<codeforces::Problem, HashMap<codeforces::Handle, codeforces::VerdictCategory>>,
    },
}

pub async fn handle(
    command: TelegramControlCommand,
    bot: Arc<Bot>,
    storage: Arc<MyStorage>,
) -> Result<()> {
    match command {
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
        SetAndNotifyDailyProblem { chat_id, problem } => {
            let mut state: ChannelState = storage
                .clone()
                .get_dialogue(chat_id)
                .await
                .into_diagnostic()?
                .unwrap_or_default();

            // archive problem
            if let (Some(current_problem), Some(current_message)) =
                (state.current_daily_problem, state.current_daily_message)
            {
                state
                    .archived_daily_messages
                    .entry(current_problem)
                    .or_default()
                    .push(current_message);
            }

            // update message
            let message = bot
                .send_message(
                    chat_id,
                    state.message_text_for_problem(&problem, &HashMap::new())?,
                )
                .await
                .into_diagnostic()?;
            state.current_daily_message = Some(message);
            // update problem
            state.current_daily_problem = Some(problem);

            storage
                .update_dialogue(chat_id, state)
                .await
                .into_diagnostic()?;
            Ok(())
        }
        UpdateSolvingStatus { chat_id, status } => {
            log::debug!(
                "Current solving status for chat {:?} is {:?}",
                chat_id,
                status
            );
            let mut state: ChannelState = storage
                .clone()
                .get_dialogue(chat_id)
                .await
                .into_diagnostic()?
                .unwrap_or_default();

            let cur_message = state.current_daily_message.as_ref().ok_or_else(|| {
                miette!("Tried to update daily message, without there beeing a daily message")
            })?;
            let new_text = state.daily_message(&status)?;
            log::debug!(
                "New text: \"{:?}\"\nOld text: \"{:?}\"",
                new_text,
                cur_message.text()
            );

            if new_text
                != cur_message
                    .text()
                    .ok_or_else(|| miette!("Daily message does not have text"))?
            {
                state.current_daily_message = Some(
                    bot.edit_message_text(chat_id, cur_message.id, state.daily_message(&status)?)
                        .await
                        .into_diagnostic()?,
                );
                storage
                    .update_dialogue(chat_id, state)
                    .await
                    .into_diagnostic()?;
            }
            Ok(())
        }
    }
}
