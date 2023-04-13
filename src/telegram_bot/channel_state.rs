use crate::codeforces;
use futures::StreamExt;
use miette::{IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::RangeInclusive;
use std::time::{SystemTime, UNIX_EPOCH};
use teloxide::prelude::*;
use xorshift::{SeedableRng, Xorshift128, Rng};

const DEFAULT_RATING_RANGE: RangeInclusive<u64> = 2000..=2400;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChannelState {
    pub(super) registered_users: HashMap<String, codeforces::Handle>,
    pub(super) rating_range: Option<RangeInclusive<u64>>,
    pub(super) current_daily_problem: Option<codeforces::Problem>,
    pub(super) current_daily_message: Option<Message>,
    pub(super) archived_daily_messages: HashMap<codeforces::Problem, Vec<Message>>,
}

impl ChannelState {
    pub fn rating_range(&self) -> &RangeInclusive<u64> {
        self.rating_range.as_ref().unwrap_or(&DEFAULT_RATING_RANGE)
    }
    pub fn registered_users(&self) -> &HashMap<String, codeforces::Handle> {
        &self.registered_users
    }
    #[allow(dead_code)]
    pub fn current_daily_problem(&self) -> &Option<codeforces::Problem> {
        &self.current_daily_problem
    }

    pub async fn known_problems(
        &self,
        cf_client: &codeforces::Client,
    ) -> HashSet<codeforces::Problem> {
        futures::stream::iter(self.registered_users().values())
            .filter_map(|handle| async {
                match handle.get_submissions(cf_client).await {
                    Ok(submissions) => Some(submissions),
                    Err(err) => {
                        log::warn!("Error getting submissions for {}\n{}", handle.as_str(), err);
                        None
                    }
                }
            })
            .flat_map(|submissions| {
                futures::stream::iter(submissions.into_iter().map(|submission| submission.problem))
            })
            .collect()
            .await
    }

    pub async fn find_daily_problem(
        &self,
        cf_client: &codeforces::Client,
        chat_id: ChatId,
    ) -> Result<codeforces::Problem> {
        let mut rng: Xorshift128 = {
            let unix_time_s = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .into_diagnostic()?
                .as_secs();
            let chat_hash = {
                let mut hasher = DefaultHasher::new();
                chat_id.hash(&mut hasher);
                hasher.finish()
            };
            let states = [unix_time_s, chat_hash];
            SeedableRng::from_seed(&states[..])
        };
        let known_problems = self.known_problems(cf_client).await;

        loop {
            let tag_index = (rng.next_u64() as usize) % codeforces::TAGS.len();
            let problems = cf_client
                .get_problems_by_tag(std::iter::once(codeforces::TAGS[tag_index]))
                .await?;
            let mut problems: Vec<_> = problems
                .into_iter()
                .filter(|problem| {
                    problem.rating.map_or(false, |rating| {
                        self.rating_range().contains(&rating)
                    }) && !known_problems.contains(problem)
                })
                .collect();
            log::debug!(
                "For tag {} there are {} admissible problems",
                codeforces::TAGS[tag_index],
                problems.len()
            );

            if !problems.is_empty() {
                return Ok(problems.swap_remove((rng.next_u64() as usize) % problems.len()));
            }
            log::warn!("Tag {} has no viable problems", codeforces::TAGS[tag_index],);
        }
    }

    pub fn message_text_for_problem(
        &self,
        problem: &codeforces::Problem,
        status: &HashMap<codeforces::Handle, codeforces::VerdictCategory>,
    ) -> Result<String> {
        let status_str = |verdict_category_opt| match verdict_category_opt {
            Some(codeforces::VerdictCategory::Correct) => "🟩️",
            Some(codeforces::VerdictCategory::JudgingNotCompleted) => "🟦️",
            Some(codeforces::VerdictCategory::Incorrect) => "🟥️",
            None => "⬜",
        };

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
                    order => order.reverse(),
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
    }
}
