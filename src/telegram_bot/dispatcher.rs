use crate::codeforces;
use crate::options::Options;
use crate::scheduler::SchedulerControlCommand;
use crate::telegram_bot::channel_state::ChannelState;
use miette::{miette, IntoDiagnostic, Result};
use std::sync::Arc;
use teloxide::dispatching::{dialogue, ShutdownToken, UpdateHandler};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use teloxide::{dptree, Bot};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

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
    #[command(description = "Register a user.\n\tUsage: /register <display-name> <cf-handle>")]
    Register {
        display_name: String,
        codeforces_handle: String,
    },
    #[command(
        rename = "setrange",
        description = "Set the considered rating range.\n\tUsage: /setrange <lower-bound> <upper-bound>"
    )]
    SetRatingRange { lower_bound: u64, upper_bound: u64 },
}

#[cfg(not(feature = "persistent"))]
pub type MyStorage = teloxide::dispatching::dialogue::InMemStorage<ChannelState>;
#[cfg(feature = "persistent")]
pub type MyStorage = teloxide::dispatching::dialogue::RedisStorage<
    teloxide::dispatching::dialogue::serializer::Bincode,
>;

pub type MyDialogue = Dialogue<ChannelState, MyStorage>;

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
        if let Some(handle) =
            codeforces::Handle::from_checked(codeforces_handle.clone(), &codeforces::Client::new())
                .await
        {
            let message_str = {
                // get and change storage
                let mut state = dialogue.get_or_default().await.into_diagnostic()?;
                state
                    .registered_users
                    .entry(display_name.clone())
                    .and_modify(|old| *old = handle.clone())
                    .or_insert(handle);

                // use storage to create answer
                let mut result = String::from("Current Registrations:\n");
                for (display_name, codeforces_handle) in &state.registered_users {
                    result.push_str("Name: ");
                    result.push_str(display_name);
                    result.push('\t');
                    result.push_str("Handle: ");
                    result.push_str(codeforces_handle.as_str());
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
            bot.send_message(
                msg.chat.id,
                format!("{} is no valid Codeforces Handle", codeforces_handle),
            )
            .await
            .into_diagnostic()?;
            Ok(())
        }
    } else {
        Err(miette!(
            "Handler for register command did not receive correct data"
        ))
    }
}

async fn set_rating_range(
    bot: Arc<Bot>,
    dialogue: MyDialogue,
    command: ChannelCommand,
    msg: Message,
) -> Result<()> {
    if let ChannelCommand::SetRatingRange {
        lower_bound,
        upper_bound,
    } = command
    {
        if lower_bound <= upper_bound {
            let mut state = dialogue.get_or_default().await.into_diagnostic()?;
            state.rating_range = Some(lower_bound..=upper_bound);
            dialogue.update(state).await.into_diagnostic()?;

            bot.send_message(msg.chat.id, "Updated rating range")
                .await
                .into_diagnostic()
                .map(|_| ())
        } else {
            bot.send_message(msg.chat.id, "Lower bound should not exceed upper bound")
                .await
                .into_diagnostic()
                .map(|_| ())
        }
    } else {
        Err(miette!(
            "Handler for set-rating command did not receive correct data"
        ))
    }
}

fn schema() -> UpdateHandler<miette::Error> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<ChannelCommand, _>()
        .branch(case![ChannelCommand::Start].endpoint(start))
        .branch(case![ChannelCommand::Help].endpoint(help))
        .branch(
            case![ChannelCommand::SetRatingRange {
                lower_bound,
                upper_bound
            }]
            .endpoint(set_rating_range),
        )
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
    storage: Arc<MyStorage>,
) -> (ShutdownToken, JoinHandle<()>) {
    let mut dispatcher = Dispatcher::builder(bot, schema())
        // storage is an Arc<_>, so cloning it keeps the connection
        .dependencies(dptree::deps![storage, sched_send])
        .build();

    let shutdown_token = dispatcher.shutdown_token();
    let join_handle = tokio::spawn(async move { dispatcher.dispatch().await });
    (shutdown_token, join_handle)
}

#[cfg(not(feature = "persistent"))]
pub async fn create_storage(_options: &Options) -> Result<Arc<MyStorage>> {
    Ok(MyStorage::new())
}
#[cfg(feature = "persistent")]
pub async fn create_storage(options: &Options) -> Result<Arc<MyStorage>> {
    MyStorage::open(
        options.redis_host.as_ref(),
        teloxide::dispatching::dialogue::serializer::Bincode,
    )
    .await
    .into_diagnostic()
}
