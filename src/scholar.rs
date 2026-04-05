use anyhow::{Context, Result};
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
struct S2RecommendationsResp {
    #[serde(rename = "recommendedPapers")]
    recommended_papers: Vec<S2PaperDetail>,
}

#[derive(Deserialize)]
struct S2ReferencesResp {
    data: Vec<S2ReferenceEntry>,
    next: Option<u64>,
}

#[derive(Deserialize)]
struct S2CitationsResp {
    data: Vec<S2CitationEntry>,
}

// ── http helpers ────────────────────────────────────────────────────────────

async fn s2_get(client: &Client, url: &str) -> Result<String> {
    crate::retry::retry_get(client, url, "semantic scholar", 3, 500).await
}

async fn s2_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let body = s2_get(client, url).await?;
    serde_json::from_str(&body).context("parsing semantic scholar response")
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// Extracts the YYMM submission prefix from a new-style arxiv ID (e.g. "2505" from "2505.18499").
/// Returns `None` for old-style IDs like "hep-th/9905111".
fn arxiv_yymm(id: &str) -> Option<&str> {
    let dot = id.find('.')?;
    let prefix = &id[..dot];
    if prefix.len() == 4 && prefix.bytes().all(|b| b.is_ascii_digit()) {
        Some(prefix)
    } else {
        None
    }
}

fn extract_arxiv_id(external_ids: Option<&HashMap<String, serde_json::Value>>) -> Option<String> {
    external_ids?
        .get("ArXiv")?
        .as_str()
        .map(std::string::ToString::to_string)
}

fn detail_to_paper(detail: S2PaperDetail) -> Option<ScholarPaper> {
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

// ── author types (internal) ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct S2AuthorSearchResult {
    #[serde(rename = "authorId")]
    author_id: String,
    name: String,
    #[serde(rename = "paperCount")]
    paper_count: Option<u64>,
    #[serde(rename = "citationCount")]
    citation_count: Option<u64>,
    #[serde(rename = "hIndex")]
    h_index: Option<u64>,
    url: Option<String>,
}

#[derive(Deserialize)]
struct S2AuthorSearchResp {
    data: Vec<S2AuthorSearchResult>,
}

#[derive(Deserialize)]
struct S2AuthorPapersResp {
    data: Vec<S2PaperDetail>,
}

// ── public api ──────────────────────────────────────────────────────────────

pub async fn fetch_scholar_meta(client: &Client, paper_id: &str) -> Result<ScholarMeta> {
    let fields = "tldr,citationCount,influentialCitationCount,referenceCount,venue,year,\
                  externalIds,openAccessPdf,publicationTypes,journal,fieldsOfStudy";
    let path = format!("/paper/ArXiv:{paper_id}?fields={fields}");
    let meta: S2PaperMeta = s2_json(client, &format!("{S2_API}{path}")).await?;
    let doi = meta
        .external_ids
        .as_ref()
        .and_then(|ids| ids.get("DOI"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let oa = meta.open_access_pdf.as_ref();
    Ok(ScholarMeta {
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
    })
}

pub async fn fetch_references(client: &Client, paper_id: &str) -> Result<Vec<ScholarPaper>> {
    let fields = "title,year,citationCount,externalIds,authors";
    let target_yymm = arxiv_yymm(paper_id);
    let mut all = Vec::new();
    let mut offset: u64 = 0;
    loop {
        let path = format!(
            "/paper/ArXiv:{paper_id}/references?fields={fields}&limit=1000&offset={offset}"
        );
        let resp: S2ReferencesResp = s2_json(client, &format!("{S2_API}{path}")).await?;
        // S2 extracts refs from the latest arxiv revision, but records the
        // original submission date.  Papers added in a revision can appear
        // as "references" despite being submitted after the target paper.
        // Drop any reference whose arxiv YYMM prefix is strictly later.
        all.extend(
            resp.data
                .into_iter()
                .filter_map(|entry| detail_to_paper(entry.cited_paper))
                .filter(|p| match (target_yymm, p.arxiv_id.as_deref().and_then(arxiv_yymm)) {
                    (Some(t), Some(r)) => r <= t,
                    _ => true,
                }),
        );
        match resp.next {
            Some(n) => offset = n,
            None => break,
        }
    }
    Ok(all)
}

pub async fn fetch_citations(
    client: &Client,
    paper_id: &str,
    limit: usize,
) -> Result<Vec<ScholarPaper>> {
    let path = format!(
        "/paper/ArXiv:{paper_id}/citations?fields=title,year,citationCount,externalIds,authors,contexts&limit={limit}"
    );
    let resp: S2CitationsResp = s2_json(client, &format!("{S2_API}{path}")).await?;
    Ok(resp
        .data
        .into_iter()
        .filter_map(citation_to_paper)
        .collect())
}

pub async fn fetch_similar(
    client: &Client,
    paper_id: &str,
    limit: usize,
) -> Result<Vec<ScholarPaper>> {
    let url = format!(
        "https://api.semanticscholar.org/recommendations/v1/papers/forpaper/ArXiv:{paper_id}\
         ?limit={limit}&fields=title,year,citationCount,externalIds,authors"
    );
    let resp: S2RecommendationsResp = s2_json(client, &url).await?;
    Ok(resp
        .recommended_papers
        .into_iter()
        .filter_map(detail_to_paper)
        .collect())
}

pub async fn search_author(client: &Client, name: &str) -> Result<crate::types::AuthorOut> {
    let encoded = crate::text::urlencode(name);
    let path = format!(
        "/author/search?query={encoded}&limit=10&fields=name,hIndex,citationCount,paperCount,url"
    );
    let resp: S2AuthorSearchResp = s2_json(client, &format!("{S2_API}{path}")).await?;
    // s2 returns fragmented profiles — pick the one with the most citations
    let author = resp
        .data
        .into_iter()
        .max_by_key(|a| a.citation_count.unwrap_or(0))
        .ok_or_else(|| anyhow::anyhow!("no author found for \"{name}\""))?;
    Ok(crate::types::AuthorOut {
        name: author.name,
        id: author.author_id,
        h_index: author.h_index,
        citation_count: author.citation_count,
        paper_count: author.paper_count,
        url: author.url,
        papers: Vec::new(),
    })
}

pub async fn fetch_author_papers(
    client: &Client,
    author_id: &str,
    limit: usize,
) -> Result<Vec<ScholarPaper>> {
    let path = format!(
        "/author/{author_id}/papers?fields=title,year,citationCount,externalIds,authors&limit={limit}&offset=0"
    );
    let resp: S2AuthorPapersResp = s2_json(client, &format!("{S2_API}{path}")).await?;
    Ok(resp
        .data
        .into_iter()
        .filter_map(detail_to_paper)
        .collect())
}
