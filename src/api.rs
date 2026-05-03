use crate::text::{
    clean_comment, clean_overview, decode_html_entities, extract_paper_id, format_date,
    normalize_ws, sanitize_bibtex, urlencode,
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
const TIMEOUT_SECS: u64 = 90;
const MAX_CONCURRENT: usize = 8;

#[derive(Clone)]
pub struct ApiClient {
    pub(crate) client: Client,
}

impl ApiClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .user_agent("alphaxiv-cli/0.5.4")
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
                .map(|txt| if raw { decode_html_entities(&txt) } else { clean_overview(&txt) })
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

        let (axiv_result, scholar_result, hf_result, oa_result) = tokio::join!(
            self.fetch_axiv_with_overview(&id, want_overview, raw),
            crate::scholar::fetch_scholar_meta(&self.client, &id),
            crate::hf::fetch_hf_enrichment(&self.client, &id),
            crate::openalex::fetch_oa_enrichment(&self.client, &id),
        );

        let (resp, overview) = axiv_result?;

        let mut warnings: Vec<String> = Vec::new();

        let scholar = scholar_result.unwrap_or_else(|e| {
            warnings.push(format!("{e:#}"));
            crate::scholar::ScholarMeta {
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
            }
        });
        let oa = oa_result.unwrap_or_else(|e| {
            warnings.push(format!("{e:#}"));
            crate::openalex::OaEnrichment {
                is_retracted: false,
                oa_status: None,
                topic: None,
                subfield: None,
            }
        });
        let hf = hf_result.unwrap_or_else(|e| {
            warnings.push(format!("{e:#}"));
            crate::hf::HfEnrichment {
                upvotes: None,
                github_url: None,
                github_stars: None,
                models: Vec::new(),
                datasets: Vec::new(),
                spaces: Vec::new(),
            }
        });

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

        let authors = resolve_authors(api_authors, g.authors.clone());

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

        let github = {
            let axiv_gh = g
                .resources
                .and_then(|r| r.github)
                .and_then(|gh| {
                    gh.url.map(|url| GithubOut {
                        url,
                        stars: gh.stars,
                        language: gh.language,
                    })
                });
            let hf_gh = hf.github_url.as_ref().map(|url| GithubOut {
                url: url.clone(),
                stars: hf.github_stars,
                language: None,
            });
            pick_best_github(axiv_gh, hf_gh)
        };

        let bibtex = g
            .citation
            .and_then(|c| c.bibtex)
            .map(|b| sanitize_bibtex(&b));

        let date = resolve_date(
            g.first_publication_date.as_deref(),
            v.publication_date.as_deref(),
        );

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
            warnings,
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
                                    let authors = resolve_authors(
                                        resp.paper.authors,
                                        resp.paper.paper_group.authors,
                                    );
                                    let date = resolve_date(
                                        resp.paper.paper_group.first_publication_date.as_deref(),
                                        resp.paper.paper_version.publication_date.as_deref(),
                                    );
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

// ── helpers ─────────────────────────────────────────────────────────────────

/// When both alphaxiv and `HuggingFace` provide a GitHub URL, prefer the one
/// that isn't an HTML project page.  `language == "HTML"` on GitHub is a
/// reliable signal — no real code repo has HTML as its primary language.
fn pick_best_github(a: Option<GithubOut>, b: Option<GithubOut>) -> Option<GithubOut> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let a_html = a.language.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("HTML"));
            let b_html = b.language.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("HTML"));
            if a_html && !b_html { Some(b) } else { Some(a) }
        }
        (a, b) => a.or(b),
    }
}

fn resolve_authors(api_authors: Vec<crate::types::ApiPaperAuthor>, group_authors: Vec<String>) -> Vec<String> {
    if api_authors.is_empty() {
        group_authors
    } else {
        api_authors.into_iter().map(|a| a.full_name).collect()
    }
}

fn resolve_date(
    group_date: Option<&str>,
    version_date: Option<&str>,
) -> Option<String> {
    group_date.or(version_date).map(crate::text::format_date)
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
                        text: if raw_text { decode_html_entities(&r.body) } else { clean_comment(&r.body) },
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
                text: if raw_text { decode_html_entities(&c.body) } else { clean_comment(&c.body) },
                upvotes: c.upvotes,
                context,
                replies,
            }
        })
        .collect()
}
