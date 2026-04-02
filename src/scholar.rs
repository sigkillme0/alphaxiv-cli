use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const S2_API: &str = "https://api.semanticscholar.org/graph/v1";

// ── output types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ScholarMeta {
    pub tldr: Option<String>,
    pub citation_count: Option<u64>,
    pub influential_citation_count: Option<u64>,
    pub reference_count: Option<u64>,
    pub venue: Option<String>,
    pub doi: Option<String>,
    pub publication_types: Vec<String>,
    pub journal_name: Option<String>,
    pub journal_volume: Option<String>,
    pub journal_pages: Option<String>,
    pub fields_of_study: Vec<String>,
    pub open_access_url: Option<String>,
    pub open_access_license: Option<String>,
}

#[derive(Serialize)]
pub struct ScholarPaper {
    pub title: String,
    pub arxiv_id: Option<String>,
    pub year: Option<u32>,
    pub citation_count: Option<u64>,
    pub authors: Vec<String>,
    pub contexts: Vec<String>,
}

// ── api response types (internal) ───────────────────────────────────────────

#[derive(Deserialize)]
struct S2Tldr {
    text: String,
}

#[derive(Deserialize)]
struct S2PaperMeta {
    tldr: Option<S2Tldr>,
    #[serde(rename = "citationCount")]
    citation_count: Option<u64>,
    #[serde(rename = "influentialCitationCount")]
    influential_citation_count: Option<u64>,
    #[serde(rename = "referenceCount")]
    reference_count: Option<u64>,
    venue: Option<String>,
    #[serde(rename = "externalIds")]
    external_ids: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "openAccessPdf")]
    open_access_pdf: Option<S2OaPdf>,
    #[serde(rename = "publicationTypes")]
    publication_types: Option<Vec<String>>,
    journal: Option<S2Journal>,
    #[serde(rename = "fieldsOfStudy")]
    fields_of_study: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct S2OaPdf {
    url: Option<String>,
    license: Option<String>,
}

#[derive(Deserialize)]
struct S2Journal {
    name: Option<String>,
    volume: Option<String>,
    pages: Option<String>,
}

#[derive(Deserialize)]
struct S2Author {
    name: String,
}

#[derive(Deserialize)]
struct S2PaperDetail {
    #[serde(rename = "paperId")]
    paper_id: Option<String>,
    title: Option<String>,
    year: Option<u32>,
    #[serde(rename = "citationCount")]
    citation_count: Option<u64>,
    #[serde(rename = "externalIds")]
    external_ids: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    authors: Vec<S2Author>,
}

#[derive(Deserialize)]
struct S2ReferenceEntry {
    #[serde(rename = "citedPaper")]
    cited_paper: S2PaperDetail,
}

#[derive(Deserialize)]
struct S2CitationEntry {
    #[serde(rename = "citingPaper")]
    citing_paper: S2PaperDetail,
    #[serde(default)]
    contexts: Vec<String>,
}

#[derive(Deserialize)]
struct S2ReferencesResp {
    data: Vec<S2ReferenceEntry>,
}

#[derive(Deserialize)]
struct S2CitationsResp {
    data: Vec<S2CitationEntry>,
}

// ── http helpers ────────────────────────────────────────────────────────────

async fn s2_get(client: &Client, path: &str) -> Result<String> {
    let url = format!("{S2_API}{path}");
    let mut last_err = String::new();
    for attempt in 0..=3u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500 * (1 << (attempt - 1)))).await;
        }
        match client.get(&url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 {
                    bail!("paper not found on semantic scholar");
                }
                if status != 429 && (400..500).contains(&status) {
                    bail!("semantic scholar returned http {status}");
                }
                if (200..300).contains(&status) {
                    return resp.text().await.context("reading body");
                }
                last_err = format!("http {status}");
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt == 3 {
                    bail!("semantic scholar request failed after retries: {last_err}");
                }
            }
        }
    }
    bail!("request failed: {last_err}")
}

async fn s2_json<T: serde::de::DeserializeOwned>(client: &Client, path: &str) -> Result<T> {
    let body = s2_get(client, path).await?;
    serde_json::from_str(&body).context("parsing semantic scholar response")
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn extract_arxiv_id(external_ids: Option<&HashMap<String, serde_json::Value>>) -> Option<String> {
    external_ids?
        .get("ArXiv")?
        .as_str()
        .map(std::string::ToString::to_string)
}

fn detail_to_paper(detail: S2PaperDetail) -> Option<ScholarPaper> {
    detail.paper_id.as_ref()?;
    let title = detail.title?;
    if title.is_empty() {
        return None;
    }
    Some(ScholarPaper {
        title,
        arxiv_id: extract_arxiv_id(detail.external_ids.as_ref()),
        year: detail.year,
        citation_count: detail.citation_count,
        authors: detail.authors.into_iter().map(|a| a.name).collect(),
        contexts: Vec::new(),
    })
}

fn citation_to_paper(entry: S2CitationEntry) -> Option<ScholarPaper> {
    let contexts = entry.contexts;
    let mut paper = detail_to_paper(entry.citing_paper)?;
    paper.contexts = contexts;
    Some(paper)
}

// ── public api ──────────────────────────────────────────────────────────────

pub async fn fetch_scholar_meta(client: &Client, paper_id: &str) -> ScholarMeta {
    let fields = "tldr,citationCount,influentialCitationCount,referenceCount,venue,year,\
                  externalIds,openAccessPdf,publicationTypes,journal,fieldsOfStudy";
    let path = format!("/paper/ArXiv:{paper_id}?fields={fields}");
    match s2_json::<S2PaperMeta>(client, &path).await {
        Ok(meta) => {
            let doi = meta
                .external_ids
                .as_ref()
                .and_then(|ids| ids.get("DOI"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let oa = meta.open_access_pdf.as_ref();
            ScholarMeta {
                tldr: meta.tldr.map(|t| t.text),
                citation_count: meta.citation_count,
                influential_citation_count: meta.influential_citation_count,
                reference_count: meta.reference_count,
                venue: meta.venue.filter(|v| !v.is_empty()),
                doi,
                publication_types: meta.publication_types.unwrap_or_default(),
                journal_name: meta.journal.as_ref().and_then(|j| j.name.clone()),
                journal_volume: meta.journal.as_ref().and_then(|j| j.volume.clone()),
                journal_pages: meta.journal.as_ref().and_then(|j| j.pages.clone()),
                fields_of_study: meta.fields_of_study.unwrap_or_default(),
                open_access_url: oa.and_then(|p| p.url.clone()).filter(|u| !u.is_empty()),
                open_access_license: oa.and_then(|p| p.license.clone()),
            }
        }
        Err(_) => ScholarMeta {
            tldr: None,
            citation_count: None,
            influential_citation_count: None,
            reference_count: None,
            venue: None,
            doi: None,
            publication_types: Vec::new(),
            journal_name: None,
            journal_volume: None,
            journal_pages: None,
            fields_of_study: Vec::new(),
            open_access_url: None,
            open_access_license: None,
        },
    }
}

pub async fn fetch_references(client: &Client, paper_id: &str) -> Result<Vec<ScholarPaper>> {
    let path = format!(
        "/paper/ArXiv:{paper_id}/references?fields=title,year,citationCount,externalIds,authors&limit=100"
    );
    let resp: S2ReferencesResp = s2_json(client, &path).await?;
    Ok(resp
        .data
        .into_iter()
        .filter_map(|entry| detail_to_paper(entry.cited_paper))
        .collect())
}

pub async fn fetch_citations(
    client: &Client,
    paper_id: &str,
    limit: usize,
) -> Result<Vec<ScholarPaper>> {
    let path = format!(
        "/paper/ArXiv:{paper_id}/citations?fields=title,year,citationCount,externalIds,authors,contexts&limit={limit}"
    );
    let resp: S2CitationsResp = s2_json(client, &path).await?;
    Ok(resp
        .data
        .into_iter()
        .filter_map(citation_to_paper)
        .collect())
}
