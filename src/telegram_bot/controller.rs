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
        SetAndNotifyDailyProblem { chat_id, problem: new_problem } => {
            let mut state: ChannelState = storage
                .clone()
                .get_dialogue(chat_id)
                .await
                .into_diagnostic()?
                .unwrap_or_default();

            // archive problem
            if let (Some(current_problem), Some(current_message)) =
                (&state.current_daily_problem, &state.current_daily_message)
            {
                state
                    .archived_daily_messages
                    .entry(current_problem.clone())
                    .or_insert_with(Default::default)
                    .push(current_message.clone());
            }

            // update message
            let new_message = bot
                .send_message(
                    chat_id,
                    state.message_text_for_problem(&new_problem, &HashMap::new())?,
                )
                .await
                .into_diagnostic()?;
            state.current_daily_message = Some(new_message);
            // update problem
            state.current_daily_problem = Some(new_problem);

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

            let default_map = HashMap::new();
            let saved_state = state.clone();
            let mut changed = false;

            // update current daily message
            if let (Some(daily_problem), Some(daily_message)) = (
                &state.current_daily_problem,
                &mut state.current_daily_message,
            ) {
                changed |= update_message(
                    &saved_state,
                    daily_problem,
                    status.get(daily_problem).unwrap_or(&default_map),
                    &bot,
                    daily_message,
                )
                .await?;
            }

            // update archived messages
            for (problem, messages) in state.archived_daily_messages.iter_mut() {
                for message in messages.iter_mut() {
                    changed |= update_message(
                        &saved_state,
                        problem,
                        status.get(problem).unwrap_or(&default_map),
                        &bot,
                        message,
                    )
                    .await?;
                }
            }

            if changed {
                storage
                    .update_dialogue(chat_id, state)
                    .await
                    .into_diagnostic()?;
            }
            Ok(())
        }
    }
}

async fn update_message(
    channel: &ChannelState,
    problem: &codeforces::Problem,
    status: &HashMap<codeforces::Handle, codeforces::VerdictCategory>,
    bot: &Bot,
    message: &mut Message,
) -> Result<bool> {
    log::trace!("update_message:\n\tproblem({:?})\n\tstatus({:?})", problem.identifier()?, status);
    let new_text = channel.message_text_for_problem(problem, status)?;

    if new_text
        == message
            .text()
            .ok_or_else(|| miette!("Tried updating message without text"))?
    {
        log::trace!(
            "Message {:?} in {:?} for problem {:?} does not need to be changed.\nOld: {:?}\nNew:{:?}",
            message.id,
            message.chat.id,
            problem,
            message.text().unwrap(),
            new_text,
        );
        return Ok(false);
    }

    log::debug!(
        "Changing message {:?} in {:?} from {:?} to {:?}",
        message.id,
        message.chat.id,
        message.text().unwrap(),
        new_text
    );
    // update message
    *message = bot
        .edit_message_text(message.chat.id, message.id, new_text)
        .await
        .into_diagnostic()?;
    Ok(true)
}
