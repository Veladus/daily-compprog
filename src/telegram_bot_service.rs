use std::collections::HashMap;
use std::fmt::format;
use teloxide::{prelude::*, utils::command::BotCommands};
use std::sync::Arc;
use async_cron_scheduler::{Job, Scheduler};
use chrono::Local;
use log::info;
use miette::{Error, IntoDiagnostic, miette, Result, WrapErr};
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::dispatching::{dialogue, UpdateHandler};
use tokio_graceful_shutdown::SubsystemHandle;
use crate::options;

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "These commands are supported:", parse_with = "split")]
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
struct ChannelState {
    registered_users: HashMap<String, String>,
}

type MyStorage = InMemStorage<ChannelState>;
type MyDialogue = Dialogue<ChannelState, MyStorage>;

async fn start(bot: Bot, dialogue: MyDialogue, msg: Message) -> Result<()> {
    bot.send_dice(msg.chat.id).await.into_diagnostic()?;
    Ok(())
}

async fn help(bot: Bot, dialogue: MyDialogue, msg: Message) -> Result<()> {
    bot.send_message(msg.chat.id, ChannelCommand::descriptions().to_string()).await.into_diagnostic()?;
    Ok(())
}

async fn register(bot: Bot, dialogue: MyDialogue, command: ChannelCommand, msg: Message) -> Result<()> {
    if let ChannelCommand::Register { display_name, codeforces_handle } = command {
        let message_str = {
            // get and change storage
            let mut state = dialogue.get_or_default().await.into_diagnostic()?;
            state.registered_users.entry(display_name.clone()).and_modify(|old| *old = codeforces_handle.clone()).or_insert(codeforces_handle);

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

        bot.send_message(msg.chat.id, message_str).await.into_diagnostic()?;

        Ok(())
    } else {
        Err(miette!("Handler for register command did not receive correct data"))
    }
}

fn schema() -> UpdateHandler<miette::Error> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<ChannelCommand, _>()
        .branch(case![ChannelCommand::Start].endpoint(start))
        .branch(case![ChannelCommand::Help].endpoint(help))
        .branch(case![ChannelCommand::Register {display_name, codeforces_handle}].endpoint(register));

    let message_handler = Update::filter_message().branch(command_handler);

    dialogue::enter::<Update, MyStorage, ChannelState, _>().branch(message_handler)
}

pub async fn subsystem_handler(options: Arc<options::Options>, subsys: SubsystemHandle) -> Result<()> {
    log::info!("Starting Telegram Bot...");

    // setup bot
    let bot = Bot::from_env();
    let mut dispatcher = Dispatcher::builder(bot, schema()).dependencies(dptree::deps![MyStorage::new()]).build();
    let shutdown_token = dispatcher.shutdown_token();
    tokio::spawn(async move {
        dispatcher.dispatch().await
    });

    log::info!("Started Telegram Bot");

    subsys.on_shutdown_requested().await;

    log::info!("Shutting down Telegram Bot...");
    shutdown_token.shutdown().into_diagnostic()?.await;
    log::info!("Shut down Telegram Bot");
    Ok(())
}
