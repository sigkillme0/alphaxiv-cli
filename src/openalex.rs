use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

const OA_API: &str = "https://api.openalex.org";
const UA: &str = "alphaxiv-cli/0.4 (mailto:alphaxiv-cli@users.noreply.github.com)";

// ── api response types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OaWork {
    is_retracted: Option<bool>,
    open_access: Option<OaOpenAccess>,
    primary_topic: Option<OaPrimaryTopic>,
}

#[derive(Deserialize)]
struct OaOpenAccess {
    oa_status: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OaPrimaryTopic {
    display_name: Option<String>,
    subfield: Option<OaNamedEntity>,
    field: Option<OaNamedEntity>,
    domain: Option<OaNamedEntity>,
}

#[derive(Deserialize)]
struct OaNamedEntity {
    display_name: Option<String>,
}

// ── enrichment output ───────────────────────────────────────────────────────

pub struct OaEnrichment {
    pub is_retracted: bool,
    pub oa_status: Option<String>,
    pub topic: Option<String>,
    pub subfield: Option<String>,
}

// ── http ────────────────────────────────────────────────────────────────────

async fn oa_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let body = client
        .get(url)
        .header("User-Agent", UA)
        .send()
        .await
        .context("openalex request")?
        .text()
        .await
        .context("reading openalex body")?;
    serde_json::from_str(&body).context("parsing openalex response")
}

// ── public api ──────────────────────────────────────────────────────────────

pub async fn fetch_oa_enrichment(client: &Client, paper_id: &str) -> OaEnrichment {
    let url = format!(
        "{OA_API}/works/doi:10.48550/arXiv.{paper_id}\
         ?select=is_retracted,open_access,primary_topic"
    );
    match oa_json::<OaWork>(client, &url).await {
        Ok(work) => {
            let pt = work.primary_topic.as_ref();
            OaEnrichment {
                is_retracted: work.is_retracted.unwrap_or(false),
                oa_status: work.open_access.and_then(|oa| oa.oa_status),
                topic: pt.and_then(|t| t.display_name.clone()),
                subfield: pt.and_then(|t| t.subfield.as_ref()?.display_name.clone()),
            }
        }
        Err(_) => OaEnrichment {
            is_retracted: false,
            oa_status: None,
            topic: None,
            subfield: None,
        },
    }
}
