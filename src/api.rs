use crate::text::{
    clean_comment, clean_overview, extract_paper_id, format_date, normalize_ws, sanitize_bibtex,
    urlencode,
};
use crate::types::{
    ApiFeedResp, ApiOverviewResp, ApiPaperResp, ApiSearchHit, BatchEntry, CommentOut, FeedEntry,
    GithubOut, HfDatasetOut, HfModelOut, HfSpaceOut, HuggingFaceOut, JournalOut, OpenAccessOut,
    PaperOut, ReplyOut, SearchOut,
};
use anyhow::{bail, Context, Result};
use reqwest::Client;
use std::time::Duration;

const API: &str = "https://api.alphaxiv.org";
pub const SITE: &str = "https://www.alphaxiv.org";
const TIMEOUT_SECS: u64 = 30;
const MAX_CONCURRENT: usize = 8;

#[derive(Clone)]
pub struct ApiClient {
    pub(crate) client: Client,
}

impl ApiClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .user_agent("alphaxiv-cli/0.5")
            .build()
            .context("building http client")?;
        Ok(Self { client })
    }

    async fn get(&self, path: &str) -> Result<String> {
        let url = format!("{API}{path}");
        crate::retry::retry_get(&self.client, &url, "alphaxiv", 3, 500).await
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let body = self.get(path).await?;
        match serde_json::from_str::<T>(&body) {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(msg) = v.pointer("/error/message").and_then(|m| m.as_str()) {
                        bail!("{msg}");
                    }
                }
                Err(e).context("failed to parse api response")
            }
        }
    }

    // ── feed ────────────────────────────────────────────────────────────────

    pub async fn fetch_feed(
        &self,
        page: usize,
        limit: usize,
        sort: &str,
        interval: &str,
    ) -> Result<Vec<FeedEntry>> {
        let path = format!(
            "/papers/v3/feed?pageNum={}&pageSize={}&sort={}&interval={}",
            page,
            limit,
            urlencode(sort),
            urlencode(interval),
        );
        let resp: ApiFeedResp = self.json(&path).await?;
        Ok(resp
            .papers
            .into_iter()
            .map(|p| {
                let views = p
                    .metrics
                    .as_ref()
                    .and_then(|m| m.visits_count.as_ref())
                    .and_then(|v| v.all)
                    .unwrap_or(0);
                let likes = p
                    .metrics
                    .as_ref()
                    .and_then(|m| m.public_total_votes)
                    .unwrap_or(0);
                let organization = p.organization_info.first().map(|o| o.name.clone());
                FeedEntry {
                    title: normalize_ws(&p.title),
                    id: p.universal_paper_id.clone(),
                    authors: p.authors,
                    organization,
                    date: p.first_publication_date.as_deref().map(format_date),
                    views,
                    likes,
                    summary: p.paper_summary.and_then(|s| s.summary),
                    topics: p.topics,
                    github_url: p.github_url,
                    url: format!("{}/abs/{}", SITE, p.universal_paper_id),
                }
            })
            .collect())
    }

    // ── paper ───────────────────────────────────────────────────────────────

    async fn fetch_axiv_with_overview(
        &self,
        id: &str,
        want_overview: bool,
        raw: bool,
    ) -> Result<(ApiPaperResp, Option<String>)> {
        let resp: ApiPaperResp = self.json(&format!("/papers/v3/legacy/{id}")).await?;
        let overview = if want_overview {
            self.fetch_overview(&resp.paper.paper_version.id)
                .await
                .map(|txt| clean_overview(&txt, raw))
        } else {
            None
        };
        Ok((resp, overview))
    }

    pub async fn fetch_paper(
        &self,
        raw_id: &str,
        want_overview: bool,
        want_comments: bool,
        raw: bool,
    ) -> Result<PaperOut> {
        let id = extract_paper_id(raw_id);

        let (axiv_result, scholar, hf, oa) = tokio::join!(
            self.fetch_axiv_with_overview(&id, want_overview, raw),
            crate::scholar::fetch_scholar_meta(&self.client, &id),
            crate::hf::fetch_hf_enrichment(&self.client, &id),
            crate::openalex::fetch_oa_enrichment(&self.client, &id),
        );

        let (resp, overview) = axiv_result?;
        let v = resp.paper.paper_version;
        let g = resp.paper.paper_group;
        let api_authors = resp.paper.authors;
        let organizations: Vec<String> = resp
            .paper
            .organization_info
            .into_iter()
            .map(|o| o.name)
            .collect();
        let pdf_url = resp.paper.pdf_info.and_then(|pi| pi.fetcher_url);

        let authors: Vec<String> = if api_authors.is_empty() {
            g.authors.clone()
        } else {
            api_authors.into_iter().map(|a| a.full_name).collect()
        };

        let views = g
            .metrics
            .as_ref()
            .and_then(|m| m.visits_count.as_ref())
            .and_then(|v| v.all)
            .unwrap_or(0);
        let likes = g
            .metrics
            .as_ref()
            .and_then(|m| m.upvotes_count)
            .unwrap_or(0);

        let comments = if want_comments {
            process_comments(resp.comments, raw)
        } else {
            Vec::new()
        };

        let comment_count = g
            .metrics
            .as_ref()
            .and_then(|m| m.questions_count)
            .unwrap_or(comments.len() as u32);
        let reply_count: usize = comments.iter().map(|c| c.replies.len()).sum();

        let github = g
            .resources
            .and_then(|r| r.github)
            .and_then(|gh| {
                gh.url.map(|url| GithubOut {
                    url,
                    stars: gh.stars,
                    language: gh.language,
                })
            })
            .or_else(|| {
                hf.github_url.as_ref().map(|url| GithubOut {
                    url: url.clone(),
                    stars: hf.github_stars,
                    language: None,
                })
            });

        let bibtex = g
            .citation
            .and_then(|c| c.bibtex)
            .map(|b| sanitize_bibtex(&b));

        let date = g
            .first_publication_date
            .as_deref()
            .or(v.publication_date.as_deref())
            .map(format_date);

        let version = v.version_label;
        let upid = v.universal_paper_id.as_deref().unwrap_or(&id);

        let journal = scholar.journal_name.map(|name| JournalOut {
            name,
            volume: scholar.journal_volume,
            pages: scholar.journal_pages,
        });

        let open_access = {
            let has_data = scholar.open_access_url.is_some() || oa.oa_status.is_some();
            if has_data {
                Some(OpenAccessOut {
                    status: oa.oa_status,
                    pdf_url: scholar.open_access_url,
                    license: scholar.open_access_license,
                })
            } else {
                None
            }
        };

        let huggingface = HuggingFaceOut {
            paper_url: format!("{}/papers/{upid}", crate::hf::HF_SITE),
            upvotes: hf.upvotes,
            models: hf
                .models
                .into_iter()
                .map(|m| HfModelOut {
                    url: format!("{}/{}", crate::hf::HF_SITE, m.id),
                    id: m.id,
                    likes: m.likes,
                    downloads: m.downloads,
                    pipeline: m.pipeline,
                })
                .collect(),
            datasets: hf
                .datasets
                .into_iter()
                .map(|d| HfDatasetOut {
                    url: format!("{}/datasets/{}", crate::hf::HF_SITE, d.id),
                    id: d.id,
                    likes: d.likes,
                    downloads: d.downloads,
                })
                .collect(),
            spaces: hf
                .spaces
                .into_iter()
                .map(|sp| HfSpaceOut {
                    url: format!("{}/spaces/{}", crate::hf::HF_SITE, sp.id),
                    id: sp.id,
                    likes: sp.likes,
                })
                .collect(),
        };

        Ok(PaperOut {
            title: normalize_ws(&v.title),
            id: upid.to_string(),
            version,
            authors,
            organizations,
            date,
            abstract_text: v.abstract_text,
            topics: g.topics,
            views,
            likes,
            comment_count,
            reply_count,
            comments,
            overview,
            github,
            bibtex,
            tldr: scholar.tldr,
            citation_count: scholar.citation_count,
            influential_citation_count: scholar.influential_citation_count,
            reference_count: scholar.reference_count,
            venue: scholar.venue,
            doi: scholar.doi,
            publication_type: scholar.publication_types.into_iter().next(),
            journal,
            fields_of_study: scholar.fields_of_study,
            open_access,
            is_retracted: oa.is_retracted,
            openalex_topic: oa.topic,
            openalex_subfield: oa.subfield,
            huggingface,
            alphaxiv_url: format!("{SITE}/abs/{upid}"),
            arxiv_url: format!("https://arxiv.org/abs/{upid}"),
            pdf_url,
        })
    }

    async fn fetch_overview(&self, version_id: &str) -> Option<String> {
        let resp: ApiOverviewResp = self
            .json(&format!("/papers/v3/{version_id}/overview/en"))
            .await
            .ok()?;
        resp.overview.filter(|s| !s.is_empty())
    }

    // ── search ──────────────────────────────────────────────────────────────

    pub async fn search_papers(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SearchOut>> {
        let path = format!(
            "/search/v2/paper/fast?q={}&includePrivate=false",
            urlencode(query)
        );
        let hits: Vec<ApiSearchHit> = self.json(&path).await?;
        let hits = match limit {
            Some(n) => &hits[..n.min(hits.len())],
            None => &hits,
        };
        let work: Vec<_> = hits
            .iter()
            .map(|h| {
                let clean = normalize_ws(
                    h.title
                        .trim_start_matches(&format!("[{}] ", h.paper_id))
                        .trim_end_matches(" - arXiv")
                        .trim_end_matches(" - arXiv.org"),
                );
                (h.paper_id.clone(), clean)
            })
            .collect();

        let mut results = Vec::with_capacity(work.len());
        for chunk in work.chunks(MAX_CONCURRENT) {
            let futs: Vec<_> = chunk
                .iter()
                .map(|(paper_id, clean_title)| {
                    let api = self.clone();
                    let paper_id = paper_id.clone();
                    let clean_title = clean_title.clone();
                    async move {
                        let (abstract_text, authors, date) =
                            match api
                                .json::<ApiPaperResp>(&format!(
                                    "/papers/v3/legacy/{paper_id}"
                                ))
                                .await
                            {
                                Ok(resp) => {
                                    let abs =
                                        if resp.paper.paper_version.abstract_text.is_empty() {
                                            None
                                        } else {
                                            Some(resp.paper.paper_version.abstract_text)
                                        };
                                    let authors: Vec<String> =
                                        if resp.paper.authors.is_empty() {
                                            resp.paper.paper_group.authors
                                        } else {
                                            resp.paper
                                                .authors
                                                .into_iter()
                                                .map(|a| a.full_name)
                                                .collect()
                                        };
                                    let date = resp
                                        .paper
                                        .paper_group
                                        .first_publication_date
                                        .as_deref()
                                        .or(
                                            resp.paper
                                                .paper_version
                                                .publication_date
                                                .as_deref(),
                                        )
                                        .map(format_date);
                                    (abs, authors, date)
                                }
                                Err(_) => (None, Vec::new(), None),
                            };
                        SearchOut {
                            title: clean_title,
                            id: paper_id.clone(),
                            authors,
                            date,
                            abstract_text,
                            categories: Vec::new(),
                            url: format!("{SITE}/abs/{paper_id}"),
                        }
                    }
                })
                .collect();
            results.extend(futures::future::join_all(futs).await);
        }
        Ok(results)
    }

    // ── batch ───────────────────────────────────────────────────────────────

    pub async fn fetch_batch(
        &self,
        ids: &[String],
        overview: bool,
        comments: bool,
        raw: bool,
    ) -> Vec<BatchEntry> {
        let mut results = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MAX_CONCURRENT) {
            let futs: Vec<_> = chunk
                .iter()
                .map(|raw_id| {
                    let api = self.clone();
                    let raw_id = raw_id.clone();
                    async move {
                        let clean_id = extract_paper_id(&raw_id);
                        match api.fetch_paper(&raw_id, overview, comments, raw).await {
                            Ok(paper) => BatchEntry {
                                id: clean_id,
                                paper: Some(paper),
                                error: None,
                            },
                            Err(e) => BatchEntry {
                                id: clean_id,
                                paper: None,
                                error: Some(format!("{e:#}")),
                            },
                        }
                    }
                })
                .collect();
            results.extend(futures::future::join_all(futs).await);
        }
        results
    }
}

// ── comment processing ──────────────────────────────────────────────────────

fn process_comments(raw: Vec<crate::types::ApiComment>, raw_text: bool) -> Vec<CommentOut> {
    raw.into_iter()
        .filter(|c| c.parent_comment_id.is_none())
        .map(|c| {
            let author = c
                .author
                .as_ref()
                .and_then(|a| a.real_name.clone().or_else(|| a.username.clone()))
                .unwrap_or_else(|| "anon".into());
            let date = c.date.as_deref().map(format_date);
            let context = c
                .annotation
                .and_then(|a| a.selected_text)
                .filter(|s| !s.is_empty());
            let replies: Vec<ReplyOut> = c
                .responses
                .into_iter()
                .map(|r| {
                    let rauthor = r
                        .author
                        .as_ref()
                        .and_then(|a| a.real_name.clone().or_else(|| a.username.clone()))
                        .unwrap_or_else(|| "anon".into());
                    ReplyOut {
                        author: rauthor,
                        date: r.date.as_deref().map(format_date),
                        text: clean_comment(&r.body, raw_text),
                        upvotes: r.upvotes,
                    }
                })
                .collect();
            let title = c
                .title
                .filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("comment"));
            CommentOut {
                author,
                date,
                title,
                text: clean_comment(&c.body, raw_text),
                upvotes: c.upvotes,
                context,
                replies,
            }
        })
        .collect()
}
