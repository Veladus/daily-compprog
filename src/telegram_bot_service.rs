use crate::options;
use crate::scheduled_service::SchedulerControlCommand;
use miette::{miette, IntoDiagnostic, Result};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::dispatching::{dialogue, UpdateHandler};
use teloxide::{prelude::*, utils::command::BotCommands};
use tokio::sync::mpsc;
use tokio_graceful_shutdown::SubsystemHandle;

#[derive(Debug, Clone)]
pub enum TelegramControlCommand {}

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

#[derive(Clone, Debug, Default)]
pub struct ChannelState {
    registered_users: HashMap<String, String>,
}

type MyStorage = InMemStorage<ChannelState>;
type MyDialogue = Dialogue<ChannelState, MyStorage>;

async fn start(
    bot: Bot,
    sched_send: mpsc::UnboundedSender<SchedulerControlCommand>,
    msg: Message,
) -> Result<()> {
    sched_send
        .send(SchedulerControlCommand::StartDailyMessages {
            chat_id: msg.chat.id,
        })
        .into_diagnostic()?;
    bot.send_dice(msg.chat.id).await.into_diagnostic()?;
    Ok(())
}

async fn help(bot: Bot, msg: Message) -> Result<()> {
    bot.send_message(msg.chat.id, ChannelCommand::descriptions().to_string())
        .await
        .into_diagnostic()?;
    Ok(())
}

async fn register(
    bot: Bot,
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

pub async fn subsystem_handler(
    options: Arc<options::Options>,
    telegram_recv: mpsc::UnboundedReceiver<TelegramControlCommand>,
    sched_send: mpsc::UnboundedSender<SchedulerControlCommand>,
    subsys: SubsystemHandle,
) -> Result<()> {
    log::info!("Starting Telegram Bot...");

    // setup bot
    let bot = Bot::from_env();
    let mut dispatcher = Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![MyStorage::new(), sched_send])
        .build();
    let shutdown_token = dispatcher.shutdown_token();
    let join_handle = tokio::spawn(async move { dispatcher.dispatch().await });

    log::info!("Started Telegram Bot");

    // wait for telegram client to end (by panic), or shutdown request
    let join_error = tokio::select! {
        _ = subsys.on_shutdown_requested() => Ok(()),
        return_value = join_handle => return_value,
    };
    if let Err(error) = join_error {
        log::error!("Telegram bot terminated with error:\n{}", &error);
        subsys.request_global_shutdown();
        return Err(error).into_diagnostic();
    }

    log::info!("Shutting down Telegram Bot...");
    shutdown_token.shutdown().into_diagnostic()?.await;
    log::info!("Shut down Telegram Bot");
    Ok(())
}
