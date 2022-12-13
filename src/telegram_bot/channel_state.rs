use crate::codeforces;
use miette::{miette, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use teloxide::prelude::*;

const DEFAULT_RATING_RANGE: RangeInclusive<u64> = 2000..=2400;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChannelState {
    pub(super) registered_users: HashMap<String, codeforces::Handle>,
    pub(super) rating_range: Option<RangeInclusive<u64>>,
    pub(super) current_daily_problem: Option<codeforces::Problem>,
    pub(super) current_daily_message: Option<Message>,
}

impl ChannelState {
    pub fn rating_range(&self) -> &RangeInclusive<u64> {
        self.rating_range.as_ref().unwrap_or(&DEFAULT_RATING_RANGE)
    }
    pub fn registered_users(&self) -> &HashMap<String, codeforces::Handle> {
        &self.registered_users
    }
    pub fn current_daily_problem(&self) -> &Option<codeforces::Problem> {
        &self.current_daily_problem
    }

    pub fn daily_message(
        &self,
        status: &HashMap<codeforces::Handle, codeforces::VerdictCategory>,
    ) -> Result<String> {
        let status_str = |verdict_category_opt| match verdict_category_opt {
            Some(codeforces::VerdictCategory::Correct) => "🟩️",
            Some(codeforces::VerdictCategory::JudgingNotCompleted) => "🟦️",
            Some(codeforces::VerdictCategory::Incorrect) => "🟥️",
            None => "⬜",
        };

        if let Some(problem) = &self.current_daily_problem {
            let mut message = format!("Today's problem is: {}", problem.url()?);

            if !self.registered_users.is_empty() {
                message.push_str("\n\n");

                let mut data: Vec<_> = self
                    .registered_users
                    .iter()
                    .map(|(display_name, handle)| (status.get(handle).copied(), display_name))
                    .collect();
                data.sort_unstable_by(|(verdict1, name1), (verdict2, name2)| {
                    match verdict1.cmp(verdict2) {
                        Ordering::Equal => name1.cmp(name2),
                        order @ _ => order.reverse(),
                    }
                });

                for (verdict_category_opt, display_name) in data {
                    message.push_str(status_str(verdict_category_opt));
                    message.push(' ');
                    message.push_str(display_name);
                    message.push('\n');
                }
            }

            Ok(message.trim_end().into())
        } else {
            Err(miette!(
                "Tried to get daily message for a channel which does not have an active daily problem"
            ))
        }
    }
}
