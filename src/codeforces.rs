use miette::{IntoDiagnostic, Result};
use serde::de::DeserializeOwned;
use serde::*;

pub const BASE_URL: &str = "https://codeforces.com/api/";

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Problem {
    index: String,
    name: String,
    tags: Vec<String>,
    rating: Option<u64>,
    #[serde(rename = "contestId")]
    contest_id: Option<u64>,
    #[serde(rename = "problemsetName")]
    problemset_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct User {
    handle: String,
    rating: u64,
    #[serde(rename = "maxRating")]
    max_rating: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Member {
    handle: String,
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Party {
    #[serde(rename = "contestId")]
    contest_id: Option<String>,
    members: Vec<Member>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub struct Submission {
    id: u64,
    #[serde(rename = "contestId")]
    contest_id: u64,
    problem: Problem,
    author: Party,
    verdict: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
struct ListResponse<T> {
    status: String,
    results: Vec<T>,
}

#[derive(Debug, Clone)]
pub struct Client {
    reqwest_client: reqwest::Client,
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
        let url = format!("{BASE_URL}user.status");
        self.call(&url, &[("handle", handle)]).await
    }

    pub async fn get_problems_by_tag(
        &self,
        tags: impl Iterator<Item = &str>,
    ) -> Result<Vec<Problem>> {
        let url = format!("{BASE_URL}problemset.problems");
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
