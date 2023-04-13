use crate::scheduler::MyScheduler;
use async_cron_scheduler::{Job, JobId};
use miette::{IntoDiagnostic, Result};

pub(super) async fn register_to_schedule(
    cron_str: &str,
    scheduler: &mut MyScheduler,
    command: impl Fn(JobId) + Send + Sync + 'static,
) -> Result<JobId> {
    let job = Job::cron(cron_str).into_diagnostic()?;
    Ok(scheduler.insert(job, command))
}

