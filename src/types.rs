use serde::{Deserialize, Serialize};

// ── api response types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ApiFeedResp {
    pub papers: Vec<ApiFeedPaper>,
}

#[derive(Deserialize)]
pub struct ApiFeedPaper {
    pub title: String,
    #[serde(default)]
    pub universal_paper_id: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    pub metrics: Option<ApiFeedMetrics>,
    pub first_publication_date: Option<String>,
    pub paper_summary: Option<ApiSummaryBlob>,
    pub github_url: Option<String>,
    #[serde(default)]
    pub organization_info: Vec<ApiOrgInfo>,
}

#[derive(Deserialize)]
pub struct ApiOrgInfo {
    pub name: String,
}

#[derive(Deserialize)]
pub struct ApiFeedMetrics {
    pub visits_count: Option<ApiVisitsShort>,
    pub public_total_votes: Option<u64>,
}

#[derive(Deserialize)]
pub struct ApiVisitsShort {
    pub all: Option<u64>,
}

#[derive(Deserialize)]
pub struct ApiSummaryBlob {
    pub summary: Option<String>,
}

#[derive(Deserialize)]
pub struct ApiPaperResp {
    pub paper: ApiPaperInner,
    #[serde(default)]
    pub comments: Vec<ApiComment>,
}

#[derive(Deserialize)]
pub struct ApiPaperInner {
    pub paper_version: ApiPaperVersion,
    pub paper_group: ApiPaperGroup,
    #[serde(default)]
    pub authors: Vec<ApiPaperAuthor>,
    #[serde(default)]
    pub organization_info: Vec<ApiOrgInfo>,
    pub pdf_info: Option<ApiPdfInfo>,
}

#[derive(Deserialize)]
pub struct ApiPaperAuthor {
    pub full_name: String,
}

#[derive(Deserialize)]
pub struct ApiPaperVersion {
    pub id: String,
    pub title: String,
    #[serde(rename = "abstract", default)]
    pub abstract_text: String,
    pub publication_date: Option<String>,
    pub universal_paper_id: Option<String>,
    pub version_label: Option<String>,
}

#[derive(Deserialize)]
pub struct ApiPaperGroup {
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub metrics: Option<ApiGroupMetrics>,
    pub first_publication_date: Option<String>,
    pub resources: Option<ApiResources>,
    pub citation: Option<ApiCitation>,
}

#[derive(Deserialize)]
pub struct ApiResources {
    pub github: Option<ApiGithubResource>,
}

#[derive(Deserialize)]
pub struct ApiGithubResource {
    pub url: Option<String>,
    pub language: Option<String>,
    pub stars: Option<u64>,
}

#[derive(Deserialize)]
pub struct ApiCitation {
    pub bibtex: Option<String>,
}

#[derive(Deserialize)]
pub struct ApiPdfInfo {
    pub fetcher_url: Option<String>,
}

#[derive(Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct ApiGroupMetrics {
    pub questions_count: Option<u32>,
    pub upvotes_count: Option<u64>,
    pub visits_count: Option<ApiVisitsDetail>,
}

#[derive(Deserialize)]
pub struct ApiVisitsDetail {
    pub all: Option<u64>,
}

#[derive(Deserialize)]
pub struct ApiComment {
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub upvotes: u32,
    pub title: Option<String>,
    pub author: Option<ApiCommentAuthor>,
    pub annotation: Option<ApiAnnotation>,
    #[serde(rename = "parentCommentId")]
    pub parent_comment_id: Option<String>,
    #[serde(default)]
    pub responses: Vec<Self>,
    pub date: Option<String>,
}

#[derive(Deserialize)]
pub struct ApiCommentAuthor {
    #[serde(rename = "realName")]
    pub real_name: Option<String>,
    pub username: Option<String>,
}

#[derive(Deserialize)]
pub struct ApiAnnotation {
    #[serde(rename = "selectedText")]
    pub selected_text: Option<String>,
}

#[derive(Deserialize)]
pub struct ApiSearchHit {
    #[serde(rename = "paperId")]
    pub paper_id: String,
    pub title: String,
    #[allow(dead_code)]
    pub snippet: Option<String>,
}

#[derive(Deserialize)]
pub struct ApiOverviewResp {
    pub overview: Option<String>,
}

// ── output types ────────────────────────────────────────────────────────────
// no skip_serializing_if — every field always present for consistent schemas.
// llm agents need predictable json, not shape-shifting garbage.

#[derive(Serialize)]
pub struct FeedEntry {
    pub title: String,
    pub id: String,
    pub authors: Vec<String>,
    pub organization: Option<String>,
    pub date: Option<String>,
    pub views: u64,
    pub likes: u64,
    pub summary: Option<String>,
    pub topics: Vec<String>,
    pub github_url: Option<String>,
    pub url: String,
}

#[derive(Serialize)]
pub struct PaperOut {
    pub title: String,
    pub id: String,
    pub version: Option<String>,
    pub authors: Vec<String>,
    pub organizations: Vec<String>,
    pub date: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub topics: Vec<String>,
    pub views: u64,
    pub likes: u64,
    pub comment_count: u32,
    pub reply_count: usize,
    pub comments: Vec<CommentOut>,
    pub overview: Option<String>,
    pub github: Option<GithubOut>,
    pub bibtex: Option<String>,
    pub tldr: Option<String>,
    pub citation_count: Option<u64>,
    pub influential_citation_count: Option<u64>,
    pub reference_count: Option<u64>,
    pub venue: Option<String>,
    pub doi: Option<String>,
    pub publication_type: Option<String>,
    pub journal: Option<JournalOut>,
    pub fields_of_study: Vec<String>,
    pub open_access: Option<OpenAccessOut>,
    pub is_retracted: bool,
    pub openalex_topic: Option<String>,
    pub openalex_subfield: Option<String>,
    pub huggingface: HuggingFaceOut,
    pub alphaxiv_url: String,
    pub arxiv_url: String,
    pub pdf_url: Option<String>,
}

#[derive(Serialize)]
pub struct GithubOut {
    pub url: String,
    pub stars: Option<u64>,
    pub language: Option<String>,
}

#[derive(Serialize)]
pub struct CommentOut {
    pub author: String,
    pub date: Option<String>,
    pub title: Option<String>,
    pub text: String,
    pub upvotes: u32,
    pub context: Option<String>,
    pub replies: Vec<ReplyOut>,
}

#[derive(Serialize)]
pub struct ReplyOut {
    pub author: String,
    pub date: Option<String>,
    pub text: String,
    pub upvotes: u32,
}

#[derive(Serialize)]
pub struct JournalOut {
    pub name: String,
    pub volume: Option<String>,
    pub pages: Option<String>,
}

#[derive(Serialize)]
pub struct OpenAccessOut {
    pub status: Option<String>,
    pub pdf_url: Option<String>,
    pub license: Option<String>,
}

#[derive(Serialize)]
pub struct HuggingFaceOut {
    pub paper_url: String,
    pub upvotes: Option<u32>,
    pub models: Vec<HfModelOut>,
    pub datasets: Vec<HfDatasetOut>,
    pub spaces: Vec<HfSpaceOut>,
}

#[derive(Serialize)]
pub struct HfModelOut {
    pub id: String,
    pub likes: u64,
    pub downloads: u64,
    pub pipeline: Option<String>,
    pub url: String,
}

#[derive(Serialize)]
pub struct HfDatasetOut {
    pub id: String,
    pub likes: u64,
    pub downloads: u64,
    pub url: String,
}

#[derive(Serialize)]
pub struct HfSpaceOut {
    pub id: String,
    pub likes: u64,
    pub url: String,
}

#[derive(Serialize)]
pub struct SearchOut {
    pub title: String,
    pub id: String,
    pub authors: Vec<String>,
    pub date: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub categories: Vec<String>,
    pub url: String,
}

#[derive(Serialize)]
pub struct BatchEntry {
    pub id: String,
    pub paper: Option<PaperOut>,
    pub error: Option<String>,
}
