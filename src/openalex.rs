use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

use crate::scholar::ScholarPaper;

const OA_API: &str = "https://api.openalex.org";
const UA: &str = "alphaxiv-cli/0.5 (mailto:alphaxiv-cli@users.noreply.github.com)";

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
struct OaPrimaryTopic {
    display_name: Option<String>,
    subfield: Option<OaNamedEntity>,
}

#[derive(Deserialize)]
struct OaNamedEntity {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct OaRelatedWorksResp {
    related_works: Vec<String>,
}

#[derive(Deserialize)]
struct OaWorksResp {
    results: Vec<OaWorkDetail>,
}

#[derive(Deserialize)]
struct OaWorkDetail {
    title: Option<String>,
    authorships: Option<Vec<OaAuthorship>>,
    publication_year: Option<u32>,
    cited_by_count: Option<u64>,
    ids: Option<OaIds>,
}

#[derive(Deserialize)]
struct OaAuthorship {
    author: OaAuthor,
}

#[derive(Deserialize)]
struct OaAuthor {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct OaIds {
    doi: Option<String>,
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
    let resp = client
        .get(url)
        .header("User-Agent", UA)
        .send()
        .await
        .context("openalex request")?;
    let status = resp.status().as_u16();
    if status == 404 {
        anyhow::bail!("not found on openalex");
    }
    if !(200..300).contains(&status) {
        anyhow::bail!("openalex returned http {status}");
    }
    let body = resp.text().await.context("reading openalex body")?;
    serde_json::from_str(&body).context("parsing openalex response")
}

// ── public api ──────────────────────────────────────────────────────────────

pub async fn fetch_oa_enrichment(client: &Client, paper_id: &str) -> Result<OaEnrichment> {
    let url = format!(
        "{OA_API}/works/doi:10.48550/arXiv.{paper_id}\
         ?select=is_retracted,open_access,primary_topic"
    );
    let work: OaWork = oa_json(client, &url).await?;
    let pt = work.primary_topic.as_ref();
    Ok(OaEnrichment {
        is_retracted: work.is_retracted.unwrap_or(false),
        oa_status: work.open_access.and_then(|oa| oa.oa_status),
        topic: pt.and_then(|t| t.display_name.clone()),
        subfield: pt.and_then(|t| t.subfield.as_ref()?.display_name.clone()),
    })
}

pub async fn fetch_related(
    client: &Client,
    paper_id: &str,
    limit: usize,
) -> Result<Vec<ScholarPaper>> {
    // step 1: get related_works list
    let url = format!(
        "{OA_API}/works/doi:10.48550/arXiv.{paper_id}?select=related_works"
    );
    let resp: OaRelatedWorksResp = oa_json(client, &url).await?;
    if resp.related_works.is_empty() {
        return Ok(Vec::new());
    }

    // step 2: extract work IDs and batch-resolve
    let ids: Vec<&str> = resp
        .related_works
        .iter()
        .take(limit)
        .filter_map(|url| url.rsplit('/').next())
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let filter = ids.join("|");
    let batch_url = format!(
        "{OA_API}/works?filter=openalex:{filter}\
         &select=title,authorships,publication_year,cited_by_count,ids\
         &per_page={limit}"
    );
    let batch: OaWorksResp = oa_json(client, &batch_url).await?;

    Ok(batch
        .results
        .into_iter()
        .filter_map(|w| {
            let title = w.title?.trim().to_string();
            if title.is_empty() {
                return None;
            }
            let authors = w
                .authorships
                .unwrap_or_default()
                .into_iter()
                .filter_map(|a| a.author.display_name)
                .collect();
            let arxiv_id = w
                .ids
                .as_ref()
                .and_then(|ids| ids.doi.as_deref())
                .and_then(|doi| doi.strip_prefix("https://doi.org/10.48550/arXiv."))
                .map(str::to_string);
            Some(ScholarPaper {
                title,
                arxiv_id,
                year: w.publication_year,
                citation_count: w.cited_by_count,
                authors,
                contexts: Vec::new(),
            })
        })
        .collect())
}
