use crate::text::{
    clean_comment, clean_overview, extract_paper_id, format_date, normalize_ws, sanitize_bibtex,
    urlencode,
};
use crate::types::{FeedEntry, ApiFeedResp, PaperOut, ApiPaperResp, GithubOut, ApiOverviewResp, SearchOut, ApiSearchHit, BatchEntry, ApiComment, CommentOut, ReplyOut};
use anyhow::{bail, Context, Result};
use std::time::Duration;
use ureq::Agent;

const API: &str = "https://api.alphaxiv.org";
pub const SITE: &str = "https://www.alphaxiv.org";
const UA: &str = "alphaxiv-cli/0.4";
const MAX_RETRIES: u32 = 3;
const TIMEOUT_SECS: u64 = 30;
const MAX_CONCURRENT: usize = 8;

pub struct ApiClient {
    pub(crate) agent: Agent,
}

impl ApiClient {
    pub fn new() -> Self {
        let config = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
            .build();
        Self {
            agent: config.into(),
        }
    }

    fn get(&self, path: &str) -> Result<String> {
        let url = format!("{API}{path}");
        let mut last_err = String::new();
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(500 * (1 << (attempt - 1))));
            }
            match self.agent.get(&url).header("User-Agent", UA).call() {
                Ok(mut resp) => {
                    return resp
                        .body_mut()
                        .read_to_string()
                        .context("reading response body");
                }
                Err(ureq::Error::StatusCode(404)) => {
                    bail!("not found on alphaxiv");
                }
                Err(ureq::Error::StatusCode(code)) if code != 429 && code < 500 => {
                    bail!("api returned http {code}");
                }
                Err(e) => {
                    last_err = e.to_string();
                    if attempt == MAX_RETRIES {
                        bail!("request failed after {MAX_RETRIES} retries: {last_err}");
                    }
                }
            }
        }
        bail!("request failed: {last_err}")
    }

    fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let body = self.get(path)?;
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

    pub fn fetch_feed(
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
        let resp: ApiFeedResp = self.json(&path)?;
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

    pub fn fetch_paper(
        &self,
        raw_id: &str,
        want_overview: bool,
        want_comments: bool,
        raw: bool,
    ) -> Result<PaperOut> {
        let id = extract_paper_id(raw_id);
        let resp: ApiPaperResp = self.json(&format!("/papers/v3/legacy/{id}"))?;
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

        // fetch overview + scholar metadata + process comments concurrently
        let version_id = v.id.clone();
        let raw_comments = resp.comments;
        let paper_id_for_scholar = id.clone();
        let (comments, overview, scholar) = std::thread::scope(|s| {
            let overview_handle = if want_overview {
                Some(s.spawn(|| {
                    self.fetch_overview(&version_id)
                        .map(|txt| clean_overview(&txt, raw))
                }))
            } else {
                None
            };
            let scholar_handle = s.spawn(|| {
                crate::scholar::fetch_scholar_meta(&self.agent, &paper_id_for_scholar)
            });
            let comments = if want_comments {
                process_comments(raw_comments, raw)
            } else {
                Vec::new()
            };
            let overview = overview_handle.and_then(|h| h.join().unwrap());
            let scholar = scholar_handle.join().unwrap();
            (comments, overview, scholar)
        });

        let comment_count = g
            .metrics
            .as_ref()
            .and_then(|m| m.questions_count)
            .unwrap_or(comments.len() as u32);
        let reply_count: usize = comments.iter().map(|c| c.replies.len()).sum();

        let github = g.resources.and_then(|r| r.github).and_then(|gh| {
            gh.url.map(|url| GithubOut {
                url,
                stars: gh.stars,
                language: gh.language,
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
            alphaxiv_url: format!("{SITE}/abs/{upid}"),
            arxiv_url: format!("https://arxiv.org/abs/{upid}"),
            pdf_url,
        })
    }

    fn fetch_overview(&self, version_id: &str) -> Option<String> {
        let resp: ApiOverviewResp =
            self.json(&format!("/papers/v3/{version_id}/overview/en")).ok()?;
        resp.overview.filter(|s| !s.is_empty())
    }

    // ── search ──────────────────────────────────────────────────────────────

    pub fn search_papers(&self, query: &str, limit: Option<usize>) -> Result<Vec<SearchOut>> {
        let path = format!(
            "/search/v2/paper/fast?q={}&includePrivate=false",
            urlencode(query)
        );
        let hits: Vec<ApiSearchHit> = self.json(&path)?;
        let hits = match limit {
            Some(n) => &hits[..n.min(hits.len())],
            None => &hits,
        };
        // pre-compute clean titles before threading (borrows from hits)
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
        // fetch paper details in parallel, chunked to limit concurrency
        let mut results = Vec::with_capacity(work.len());
        for chunk in work.chunks(MAX_CONCURRENT) {
            let chunk_results: Vec<SearchOut> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|(paper_id, clean_title)| {
                        s.spawn(move || {
                            let (abstract_text, authors, date) = match self
                                .json::<ApiPaperResp>(&format!("/papers/v3/legacy/{paper_id}"))
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
                                        .or(resp.paper.paper_version.publication_date.as_deref())
                                        .map(format_date);
                                    (abs, authors, date)
                                }
                                Err(_) => (None, Vec::new(), None),
                            };
                            SearchOut {
                                title: clean_title.clone(),
                                id: paper_id.clone(),
                                authors,
                                date,
                                abstract_text,
                                url: format!("{SITE}/abs/{paper_id}"),
                            }
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            results.extend(chunk_results);
        }
        Ok(results)
    }

    // ── batch ───────────────────────────────────────────────────────────────

    pub fn fetch_batch(
        &self,
        ids: &[String],
        overview: bool,
        comments: bool,
        raw: bool,
    ) -> Vec<BatchEntry> {
        let mut results = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MAX_CONCURRENT) {
            let chunk_results: Vec<BatchEntry> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|raw_id| {
                        s.spawn(|| {
                            let clean_id = extract_paper_id(raw_id);
                            match self.fetch_paper(raw_id, overview, comments, raw) {
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
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            results.extend(chunk_results);
        }
        results
    }
}

// ── comment processing ──────────────────────────────────────────────────────

fn process_comments(raw: Vec<ApiComment>, raw_text: bool) -> Vec<CommentOut> {
    raw.into_iter()
        .filter(|c| c.parent_comment_id.is_none())
        .map(|c| {
            let author = c
                .author
                .as_ref()
                .and_then(|a| a.real_name.clone().or(a.username.clone()))
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
                        .and_then(|a| a.real_name.clone().or(a.username.clone()))
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
