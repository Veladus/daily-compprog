use miette::{miette, IntoDiagnostic, Result};
use serde::de::DeserializeOwned;
use serde::*;

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
    pub handle: String,
    pub rating: u64,
    #[serde(rename = "maxRating")]
    pub max_rating: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Member {
    pub handle: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Party {
    #[serde(rename = "contestId")]
    pub contest_id: Option<String>,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Submission {
    pub id: u64,
    #[serde(rename = "contestId")]
    pub contest_id: u64,
    pub problem: Problem,
    pub author: Party,
    pub verdict: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Client {
    reqwest_client: reqwest::Client,
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

impl Client {
    pub fn new() -> Self {
        Self {
            reqwest_client: reqwest::Client::new(),
        }
    }

    async fn call<T>(&self, url: &str, query_params: &[(&str, &str)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        #[derive(Debug, Clone, Deserialize, Serialize)]
        struct CallResponse<U> {
            status: String,
            result: U,
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
            "Codeforces did not complete the request"
        );
        Ok(response.result)
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
