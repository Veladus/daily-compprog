use async_cron_scheduler::cron::Schedule;
use clap::Parser;
use env_logger::Env;
use miette::{IntoDiagnostic, Result};
use std::str::FromStr;

#[derive(Parser)]
#[clap(version, about, long_about = None)]
pub struct Options {
    /// Increase verbosity, and can be used multiple times
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Cron options for message schedule
    #[arg(long, default_value_t = String::from("0 30 7 * * * *"))]
    pub messages_cron: String,

    /// Address of Redis instance
    #[cfg(feature = "persistent")]
    #[arg(long, default_value_t = String::from("redis://localhost:6379"))]
    pub redis_host: String,
}

pub fn parse() -> Result<Options> {
    let opts = Options::parse();

    let debug_level = match opts.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    env_logger::Builder::from_env(Env::default().default_filter_or(debug_level)).init();

    // check options
    Schedule::from_str(&opts.messages_cron).into_diagnostic()?;

    Ok(opts)
}
