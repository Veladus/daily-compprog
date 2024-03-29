use crate::{codeforces, options};
use miette::{IntoDiagnostic, Result, GraphicalReportHandler};
use std::{sync::Arc, borrow::Borrow};
use teloxide::Bot;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::SubsystemHandle;

mod channel_state;
mod controller;
mod dispatcher;

use crate::scheduler::SchedulerControlCommand;
pub use channel_state::ChannelState;
pub use controller::TelegramControlCommand;

pub async fn subsystem_handler(
    options: Arc<options::Options>,
    telegram_send: mpsc::UnboundedSender<TelegramControlCommand>,
    mut telegram_recv: mpsc::UnboundedReceiver<TelegramControlCommand>,
    sched_send: mpsc::UnboundedSender<SchedulerControlCommand>,
    cf_client: Arc<codeforces::Client>,
    subsys: SubsystemHandle,
) -> Result<()> {
    log::info!("Starting Telegram Bot...");

    // setup bot
    let bot = Arc::new(Bot::from_env());
    let storage = dispatcher::create_storage(options.as_ref()).await?;
    let (shutdown_token, mut join_handle) =
        dispatcher::setup(bot.clone(), telegram_send, sched_send, storage.clone(), cf_client).await;

    log::info!("Started Telegram Bot");

    let mut open_tasks = Vec::new();
    let spawn_task = |command| {
        let bot_clone = bot.clone();
        let storage_clone = storage.clone();
        tokio::spawn(async move {
            match controller::handle(command, bot_clone, storage_clone)
                .await {
                    Ok(_) => {},
                    Err(report) => {
                        let handler = GraphicalReportHandler::new();
                        let mut rendered_report = String::new();
                        handler.render_report(&mut rendered_report, report.borrow()).expect("Could not render error");
                        log::error!("Error in handling TelegramCommand.\n{}",rendered_report);
                    }
                }
        })
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
    log::info!("Shutting down Telegram Bot (waiting for dispatcher)...");
    shutdown_result?.await;
    log::info!("Shut down Telegram Bot");
    Ok(())
}
