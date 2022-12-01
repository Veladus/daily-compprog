use crate::scheduler::SchedulerControlCommand;
use miette::{miette, IntoDiagnostic, Result};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::dispatching::{dialogue, ShutdownToken, UpdateHandler};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use teloxide::{dptree, Bot};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Clone, Debug, Default)]
pub struct ChannelState {
    registered_users: HashMap<String, String>,
}

#[derive(BotCommands, Clone, Debug)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:",
    parse_with = "split"
)]
enum ChannelCommand {
    #[command(description = "Send this message.")]
    Help,
    #[command(description = "(Re)starts the bot in this channel.")]
    Start,
    #[command(description = "Register a user.\n\tUsage: /register <display_name> <cf-handle>")]
    Register {
        display_name: String,
        codeforces_handle: String,
    },
}

type MyStorage = InMemStorage<ChannelState>;
type MyDialogue = Dialogue<ChannelState, MyStorage>;

async fn start(
    bot: Arc<Bot>,
    sched_send: mpsc::UnboundedSender<SchedulerControlCommand>,
    msg: Message,
) -> Result<()> {
    sched_send
        .send(SchedulerControlCommand::StartDailyMessages {
            chat_id: msg.chat.id,
        })
        .into_diagnostic()?;
    bot.send_message(msg.chat.id, "A daily problem will be prepared for you🍴")
        .await
        .into_diagnostic()?;
    Ok(())
}

async fn help(bot: Arc<Bot>, msg: Message) -> Result<()> {
    bot.send_message(msg.chat.id, ChannelCommand::descriptions().to_string())
        .await
        .into_diagnostic()?;
    Ok(())
}

async fn register(
    bot: Arc<Bot>,
    dialogue: MyDialogue,
    command: ChannelCommand,
    msg: Message,
) -> Result<()> {
    if let ChannelCommand::Register {
        display_name,
        codeforces_handle,
    } = command
    {
        let message_str = {
            // get and change storage
            let mut state = dialogue.get_or_default().await.into_diagnostic()?;
            state
                .registered_users
                .entry(display_name.clone())
                .and_modify(|old| *old = codeforces_handle.clone())
                .or_insert(codeforces_handle);

            // use storage to create answer
            let mut result = String::new();
            for (display_name, codeforces_handle) in &state.registered_users {
                result.push_str("Name: ");
                result.push_str(display_name);
                result.push('\n');
                result.push_str("Handle: ");
                result.push_str(codeforces_handle);
                result.push('\n');
            }

            // save storage
            dialogue.update(state).await.into_diagnostic()?;

            result
        };

        bot.send_message(msg.chat.id, message_str)
            .await
            .into_diagnostic()?;

        Ok(())
    } else {
        Err(miette!(
            "Handler for register command did not receive correct data"
        ))
    }
}

fn schema() -> UpdateHandler<miette::Error> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<ChannelCommand, _>()
        .branch(case![ChannelCommand::Start].endpoint(start))
        .branch(case![ChannelCommand::Help].endpoint(help))
        .branch(
            case![ChannelCommand::Register {
                display_name,
                codeforces_handle
            }]
            .endpoint(register),
        );

    let message_handler = Update::filter_message().branch(command_handler);

    dialogue::enter::<Update, MyStorage, ChannelState, _>().branch(message_handler)
}

pub async fn setup(
    bot: Arc<Bot>,
    sched_send: mpsc::UnboundedSender<SchedulerControlCommand>,
) -> (ShutdownToken, JoinHandle<()>) {
    let mut dispatcher = Dispatcher::builder(bot.clone(), schema())
        .dependencies(dptree::deps![MyStorage::new(), sched_send])
        .build();
    let shutdown_token = dispatcher.shutdown_token();
    let join_handle = tokio::spawn(async move { dispatcher.dispatch().await });
    (shutdown_token, join_handle)
}
