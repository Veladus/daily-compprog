use governor::clock::DefaultClock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter};
use miette::{miette, IntoDiagnostic, Result};

use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::*;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

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

#[derive(Debug, Clone, Deserialize, Serialize, Hash, Ord, PartialOrd, Eq, PartialEq)]
#[serde(transparent)]
pub struct ProblemIdentifier(String);

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
    cache: Mutex<RefCell<HashMap<String, Box<dyn Any + Sync + Send>>>>,
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

    pub async fn get_submissions(&self, client: &Client) -> Result<Vec<Submission>> {
        let url = format!("{API_BASE}/user.status");
        client.call(&url, &[("handle", self.as_str())]).await
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

    pub fn identifier(&self) -> Result<ProblemIdentifier> {
        Ok(ProblemIdentifier(format!(
            "{}/{}",
            self.contest_id
                .ok_or_else(|| miette!("Don't know how to identify problem without contest_id"))?,
            self.index
        )))
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

impl Handle {}

impl Client {
    pub fn new() -> Self {
        Self {
            rate_limiter: RateLimiter::direct(Quota::with_period(Duration::from_secs(3)).unwrap()),
            reqwest_client: reqwest::Client::new(),
            cache: Mutex::new(RefCell::new(HashMap::new())),
        }
    }

    async fn call<T>(&self, url: &str, query_params: &[(&str, &str)]) -> Result<T>
    where
        T: DeserializeOwned + Clone + Sync + Send + 'static,
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
            .into_diagnostic()?;

        // handle too many requests
        if response.status() == StatusCode::from_u16(503).into_diagnostic()? {
            log::warn!("Too many requests to codeforces -- using cache");
            if let Some(cached_value) = self.cache.lock().await.borrow().get(url) {
                log::debug!("\tCached: {}", url);
                return cached_value
                    .downcast_ref::<T>()
                    .map(|cache_ref| cache_ref.clone())
                    .ok_or_else(|| miette!("Could not convert cached value to desired type"));
            } else {
                log::warn!("\tNot cached: {}", url);
            }
        }

        // handle normal response
        let call_response = response.json::<CallResponse<T>>().await.into_diagnostic()?;
        miette::ensure!(
            call_response.status == "OK",
            "Codeforces did not complete the request. Comment: {:?}",
            call_response.comment,
        );
        let result = call_response
            .result
            .ok_or_else(|| miette!("Codeforces did not provide a result"))?;

        // cache result
        self.cache
            .lock()
            .await
            .borrow_mut()
            .insert(String::from(url), Box::new(result.clone()));

        Ok(result)
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
