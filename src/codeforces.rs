use governor::clock::DefaultClock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter};
use miette::{miette, IntoDiagnostic, Result};

use serde::de::DeserializeOwned;
use serde::*;
use std::time::Duration;

pub const BASE: &str = "https://codeforces.com";
pub const API_BASE: &str = "https://codeforces.com/api";
pub const TAGS: &[&str] = &[
    "2-sat",
    "binary search",
    "bitmasks",
    "brute force",
    "chinese remainder theorem",
    "combinatorics",
    "constructive algorithms",
    "data structures",
    "dfs and similar",
    "divide and conquer",
    "dp",
    "dsu",
    "expression parsing",
    "fft",
    "flows",
    "games",
    "geometry",
    "graph matchings",
    "graphs",
    "greedy",
    "hashing",
    "implementation",
    "math",
    "matrices",
    "meet-in-the-middle",
    "number theory",
    "probabilities",
    "schedules",
    "shortest paths",
    "sortings",
    "string suffix structures",
    "strings",
    "ternary search",
    "trees",
    "two pointers",
];

#[derive(Debug, Clone, Deserialize, Serialize, Hash, Ord, PartialOrd, Eq, PartialEq)]
#[serde(transparent)]
pub struct Handle(String);

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Problem {
    pub index: String,
    pub name: String,
    pub tags: Vec<String>,
    pub rating: Option<u64>,
    #[serde(rename = "contestId")]
    pub contest_id: Option<u64>,
    #[serde(rename = "problemsetName")]
    pub problemset_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct User {
    pub handle: Handle,
    pub rating: Option<u64>,
    #[serde(rename = "maxRating")]
    pub max_rating: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct PartyMember {
    pub handle: Handle,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Party {
    #[serde(rename = "contestId")]
    pub contest_id: Option<u64>,
    pub members: Vec<PartyMember>,
}

#[derive(Debug, Clone, Copy, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum VerdictCategory {
    JudgingNotCompleted,
    Incorrect,
    Correct,
}

#[derive(Debug, Clone, Copy, Hash, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Failed,
    Ok,
    Partial,
    CompilationError,
    RuntimeError,
    WrongAnswer,
    PresentationError,
    TimeLimitExceeded,
    MemoryLimitExceeded,
    IdlenessLimitExceeded,
    SecurityViolated,
    Crashed,
    InputPreparationCrashed,
    Challenged,
    Skipped,
    Testing,
    Rejected,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Submission {
    pub id: u64,
    #[serde(rename = "contestId")]
    pub contest_id: u64,
    pub problem: Problem,
    pub author: Party,
    pub verdict: Option<Verdict>,
}

#[derive(Debug)]
pub struct Client {
    rate_limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>,
    reqwest_client: reqwest::Client,
}

impl From<String> for Handle {
    fn from(str: String) -> Self {
        Self(str)
    }
}
impl Handle {
    pub async fn from_checked(str: String, client: &Client) -> Option<Self> {
        let url = format!("{API_BASE}/user.info/");
        // check that requesting data about this handle gives an ok result
        client
            .call::<Vec<User>>(&url, &[("handles", &str)])
            .await
            .ok()
            .map(|_| Handle(str))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Problem {
    pub fn url(&self) -> Result<String> {
        Ok(format!(
            "{BASE}/contest/{}/problem/{}",
            self.contest_id.ok_or_else(|| miette!(
                "Don't know how to synthesize URL of problem without contest_id"
            ))?,
            self.index
        ))
    }
}

impl Verdict {
    pub fn category(&self) -> VerdictCategory {
        use Verdict::*;
        use VerdictCategory::*;
        match self {
            Ok => Correct,
            Partial
            | WrongAnswer
            | PresentationError
            | TimeLimitExceeded
            | MemoryLimitExceeded
            | IdlenessLimitExceeded
            | Challenged
            | RuntimeError => Incorrect,
            Failed
            | SecurityViolated
            | Crashed
            | InputPreparationCrashed
            | Rejected
            | Skipped
            | Testing
            | CompilationError => JudgingNotCompleted,
        }
    }
}

impl Client {
    pub fn new() -> Self {
        Self {
            rate_limiter: RateLimiter::direct(Quota::with_period(Duration::from_secs(2)).unwrap()),
            reqwest_client: reqwest::Client::new(),
        }
    }

    async fn call<T>(&self, url: &str, query_params: &[(&str, &str)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.rate_limiter
            .until_ready_with_jitter(Jitter::new(
                Duration::from_millis(50),
                Duration::from_millis(500),
            ))
            .await;

        #[derive(Debug, Clone, Deserialize, Serialize)]
        struct CallResponse<U> {
            status: String,
            comment: Option<String>,
            result: Option<U>,
        }

        let response = self
            .reqwest_client
            .get(url)
            .query(query_params)
            .send()
            .await
            .into_diagnostic()?
            .json::<CallResponse<T>>()
            .await
            .into_diagnostic()?;

        miette::ensure!(
            response.status == "OK",
            "Codeforces did not complete the request. Comment: {:?}",
            response.comment,
        );
        response
            .result
            .ok_or_else(|| miette!("Codeforces did not provide a result"))
    }

    pub async fn get_user_submissions(&self, handle: &str) -> Result<Vec<Submission>> {
        let url = format!("{API_BASE}/user.status");
        self.call(&url, &[("handle", handle)]).await
    }

    pub async fn get_problems_by_tag(
        &self,
        tags: impl Iterator<Item = &str>,
    ) -> Result<Vec<Problem>> {
        let url = format!("{API_BASE}/problemset.problems");
        let tags_string: String = tags.collect::<Vec<_>>().join(";");

        #[derive(Debug, Clone, Deserialize, Serialize)]
        struct CallResponse {
            problems: Vec<Problem>,
        }

        self.call::<CallResponse>(&url, &[("tags", &tags_string)])
            .await
            .map(|call_response| call_response.problems)
    }
}
