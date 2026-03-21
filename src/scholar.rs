use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const S2_API: &str = "https://api.semanticscholar.org/graph/v1";
const UA: &str = "alphaxiv-cli/0.4";

// ── output types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ScholarMeta {
    pub tldr: Option<String>,
    pub citation_count: Option<u64>,
    pub influential_citation_count: Option<u64>,
    pub reference_count: Option<u64>,
    pub venue: Option<String>,
}

#[derive(Serialize)]
pub struct ScholarPaper {
    pub title: String,
    pub arxiv_id: Option<String>,
    pub year: Option<u32>,
    pub citation_count: Option<u64>,
    pub authors: Vec<String>,
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

fn s2_get(agent: &ureq::Agent, path: &str) -> Result<String> {
    let url = format!("{S2_API}{path}");
    let mut last_err = String::new();
    for attempt in 0..=3u32 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(500 * (1 << (attempt - 1))));
        }
        match agent.get(&url).header("User-Agent", UA).call() {
            Ok(mut resp) => return resp.body_mut().read_to_string().context("reading body"),
            Err(ureq::Error::StatusCode(404)) => bail!("paper not found on semantic scholar"),
            Err(ureq::Error::StatusCode(code)) if code != 429 && code < 500 => {
                bail!("semantic scholar returned http {code}")
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt == 3 {
                    bail!(
                        "semantic scholar request failed after retries: {last_err}"
                    );
                }
            }
        }
    }
    bail!("request failed: {last_err}")
}

fn s2_json<T: serde::de::DeserializeOwned>(agent: &ureq::Agent, path: &str) -> Result<T> {
    let body = s2_get(agent, path)?;
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
    // skip papers with null paperId or missing title
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
    })
}

// ── public api ──────────────────────────────────────────────────────────────

pub fn fetch_scholar_meta(agent: &ureq::Agent, paper_id: &str) -> ScholarMeta {
    let path = format!(
        "/paper/ArXiv:{paper_id}?fields=tldr,citationCount,influentialCitationCount,referenceCount,venue,year"
    );
    match s2_json::<S2PaperMeta>(agent, &path) {
        Ok(meta) => ScholarMeta {
            tldr: meta.tldr.map(|t| t.text),
            citation_count: meta.citation_count,
            influential_citation_count: meta.influential_citation_count,
            reference_count: meta.reference_count,
            venue: meta.venue.filter(|v| !v.is_empty()),
        },
        Err(_) => ScholarMeta {
            tldr: None,
            citation_count: None,
            influential_citation_count: None,
            reference_count: None,
            venue: None,
        },
    }
}

pub fn fetch_references(agent: &ureq::Agent, paper_id: &str) -> Result<Vec<ScholarPaper>> {
    let path = format!(
        "/paper/ArXiv:{paper_id}/references?fields=title,year,citationCount,externalIds,authors&limit=100"
    );
    let resp: S2ReferencesResp = s2_json(agent, &path)?;
    Ok(resp
        .data
        .into_iter()
        .filter_map(|entry| detail_to_paper(entry.cited_paper))
        .collect())
}

pub fn fetch_citations(
    agent: &ureq::Agent,
    paper_id: &str,
    limit: usize,
) -> Result<Vec<ScholarPaper>> {
    let path = format!(
        "/paper/ArXiv:{paper_id}/citations?fields=title,year,citationCount,externalIds,authors&limit={limit}"
    );
    let resp: S2CitationsResp = s2_json(agent, &path)?;
    Ok(resp
        .data
        .into_iter()
        .filter_map(|entry| detail_to_paper(entry.citing_paper))
        .collect())
}
