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

use TelegramControlCommand::*;
#[derive(Debug, Clone)]
pub enum TelegramControlCommand {
    SendMessage { chat_id: ChatId, message: String },
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

#[derive(Clone, Debug, Default)]
pub struct ChannelState {
    registered_users: HashMap<String, String>,
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

async fn handle_control_command(command: TelegramControlCommand, bot: Arc<Bot>) -> Result<()> {
    match command {
        SendMessage { chat_id, message } => {
            bot.send_message(chat_id, message).await.into_diagnostic()?;
            Ok(())
        }
    }
}

pub async fn subsystem_handler(
    _options: Arc<options::Options>,
    mut telegram_recv: mpsc::UnboundedReceiver<TelegramControlCommand>,
    sched_send: mpsc::UnboundedSender<SchedulerControlCommand>,
    subsys: SubsystemHandle,
) -> Result<()> {
    log::info!("Starting Telegram Bot...");

    // setup bot
    let bot = Arc::new(Bot::from_env());
    let mut dispatcher = Dispatcher::builder(bot.clone(), schema())
        .dependencies(dptree::deps![MyStorage::new(), sched_send])
        .build();
    let shutdown_token = dispatcher.shutdown_token();
    let mut join_handle = tokio::spawn(async move { dispatcher.dispatch().await });

    log::info!("Started Telegram Bot");

    let mut open_tasks = Vec::new();
    let spawn_task = |command| {
        let bot_clone = bot.clone();
        tokio::spawn(async move { handle_control_command(command, bot_clone).await.unwrap() })
    };
    // wait for telegram client to end (by panic), or shutdown request
    let join_error = loop {
        tokio::select! {
            _ = subsys.on_shutdown_requested() => break Ok(()),
            return_value = &mut join_handle => break return_value,
            command_opt = telegram_recv.recv() => match command_opt {
                Some(command) => open_tasks.push(spawn_task(command)),
                None => break Ok(()),
            },
        };
    };
    if let Err(error) = join_error {
        log::error!("Telegram bot terminated with error:\n{}", &error);
        subsys.request_global_shutdown();
        return Err(error).into_diagnostic();
    }

    log::info!("Shutting down Telegram Bot...");
    let shutdown_result = shutdown_token.shutdown().into_diagnostic();

    // process pending control statements
    telegram_recv.close();
    while let Some(command) = telegram_recv.recv().await {
        open_tasks.push(spawn_task(command));
    }
    log::debug!("{} open task(s) in scheduler service", open_tasks.len());
    for handle in open_tasks {
        handle.await.into_diagnostic()?;
    }

    // wait for telegram dispatcher to terminate
    shutdown_result?.await;
    log::info!("Shut down Telegram Bot");
    Ok(())
}
