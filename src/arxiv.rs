use anyhow::{bail, Context, Result};
use reqwest::Client;
use std::time::Duration;

use crate::api::SITE;
use crate::text;
use crate::types::SearchOut;

const ARXIV_API: &str = "https://export.arxiv.org/api/query";
const MAX_RETRIES: u32 = 3;

// ── public api ──────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn search(
    client: &Client,
    query: &str,
    sort_by: &str,
    sort_order: &str,
    start: usize,
    max_results: usize,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<Vec<SearchOut>> {
    let mut search_query = if is_fielded_query(query) {
        text::urlencode(query)
    } else {
        // quote multi-word queries so arxiv treats them as a phrase
        let encoded = text::urlencode(query);
        if query.contains(' ') {
            format!("all:%22{encoded}%22")
        } else {
            format!("all:{encoded}")
        }
    };

    if let Some(df) = date_filter(date_from, date_to) {
        search_query = format!("{search_query}+AND+{df}");
    }

    let url = format!(
        "{ARXIV_API}?search_query={search_query}\
         &sortBy={sort_by}&sortOrder={sort_order}\
         &start={start}&max_results={max_results}"
    );

    let body = get(client, &url).await?;
    Ok(parse_feed(&body))
}

pub async fn browse_category(
    client: &Client,
    category: &str,
    start: usize,
    max_results: usize,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<Vec<SearchOut>> {
    let mut query = format!("cat:{}", text::urlencode(category));

    if let Some(df) = date_filter(date_from, date_to) {
        query = format!("{query}+AND+{df}");
    }

    let url = format!(
        "{ARXIV_API}?search_query={query}\
         &sortBy=submittedDate&sortOrder=descending\
         &start={start}&max_results={max_results}"
    );

    let body = get(client, &url).await?;
    Ok(parse_feed(&body))
}

// ── http ────────────────────────────────────────────────────────────────────

async fn get(client: &Client, url: &str) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(1000 * (1 << (attempt - 1)))).await;
        }
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    return resp.text().await.context("reading arxiv response");
                }
                if status == 400 {
                    bail!("arxiv api rejected the query");
                }
                if status != 429 && (400..500).contains(&status) {
                    bail!("arxiv api returned http {status}");
                }
                last_err = format!("http {status}");
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt == MAX_RETRIES {
                    bail!("arxiv request failed after retries: {last_err}");
                }
            }
        }
    }
    bail!("arxiv request failed: {last_err}")
}

// ── atom xml parsing ────────────────────────────────────────────────────────

fn parse_feed(xml: &str) -> Vec<SearchOut> {
    let mut results = Vec::new();
    let mut pos = 0;

    while let Some(start) = xml[pos..].find("<entry>") {
        let entry_start = pos + start + 7;
        let Some(end) = xml[entry_start..].find("</entry>") else {
            break;
        };
        let entry_xml = &xml[entry_start..entry_start + end];
        pos = entry_start + end + 8;

        if let Some(entry) = parse_entry(entry_xml) {
            results.push(entry);
        }
    }

    results
}

fn parse_entry(xml: &str) -> Option<SearchOut> {
    let raw_id = tag_content(xml, "id")?;

    // error entries have ids containing "/errors"
    if raw_id.contains("/errors") {
        return None;
    }

    let id = id_from_url(&raw_id);
    if id.is_empty() {
        return None;
    }

    let title = tag_content(xml, "title")
        .map(|t| text::normalize_ws(&text::decode_html_entities(&t)))
        .unwrap_or_default();

    if title.is_empty() {
        return None;
    }

    let abstract_text = tag_content(xml, "summary")
        .map(|s| text::normalize_ws(&text::decode_html_entities(&s)))
        .filter(|s| !s.is_empty());

    let date = tag_content(xml, "published")
        .as_deref()
        .map(text::format_date);

    let authors = extract_authors(xml);
    let categories = extract_categories(xml);

    Some(SearchOut {
        title,
        id: id.clone(),
        authors,
        date,
        abstract_text,
        categories,
        url: format!("{SITE}/abs/{id}"),
    })
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// Extract text between `<tag>...</tag>` or `<tag attr>...</tag>`.
fn tag_content(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?;
    let gt = start + xml[start..].find('>')?;
    let content_start = gt + 1;
    let content_end = content_start + xml[content_start..].find(&close)?;
    Some(xml[content_start..content_end].to_string())
}

fn extract_authors(xml: &str) -> Vec<String> {
    let mut authors = Vec::new();
    let mut pos = 0;
    while let Some(start) = xml[pos..].find("<author>") {
        let a_start = pos + start;
        let Some(end) = xml[a_start..].find("</author>") else {
            break;
        };
        let author_block = &xml[a_start..a_start + end];
        pos = a_start + end + 9;

        if let Some(name) = tag_content(author_block, "name") {
            let name = text::normalize_ws(&text::decode_html_entities(&name));
            if !name.is_empty() {
                authors.push(name);
            }
        }
    }
    authors
}

fn extract_categories(xml: &str) -> Vec<String> {
    let mut cats = Vec::new();
    let mut pos = 0;
    while let Some(start) = xml[pos..].find("<category ") {
        let abs_start = pos + start;
        let Some(end) = xml[abs_start..].find("/>").or_else(|| xml[abs_start..].find('>')) else {
            break;
        };
        let tag = &xml[abs_start..abs_start + end];
        pos = abs_start + end + 2;
        if let Some(t_start) = tag.find("term=\"") {
            let val_start = t_start + 6;
            if let Some(t_end) = tag[val_start..].find('"') {
                let term = &tag[val_start..val_start + t_end];
                if !term.is_empty() {
                    cats.push(term.to_string());
                }
            }
        }
    }
    cats
}

/// `http://arxiv.org/abs/2502.11089v1` -> `2502.11089`
fn id_from_url(url: &str) -> String {
    let s = url.trim();
    let id = s
        .strip_prefix("http://arxiv.org/abs/")
        .or_else(|| s.strip_prefix("https://arxiv.org/abs/"))
        .unwrap_or(s);
    // strip version suffix
    if let Some(v) = id.rfind('v') {
        if !id[v + 1..].is_empty() && id[v + 1..].bytes().all(|b| b.is_ascii_digit()) {
            return id[..v].to_string();
        }
    }
    id.to_string()
}

fn is_fielded_query(q: &str) -> bool {
    let prefixes = ["ti:", "au:", "abs:", "co:", "jr:", "cat:", "rn:", "all:"];
    let ops = [" AND ", " OR ", " ANDNOT "];
    prefixes.iter().any(|p| q.contains(p)) || ops.iter().any(|op| q.contains(op))
}

/// Convert optional ISO dates to arxiv submittedDate range filter.
fn date_filter(from: Option<&str>, to: Option<&str>) -> Option<String> {
    if from.is_none() && to.is_none() {
        return None;
    }
    let f = from.map_or_else(
        || "199101010000".to_string(),
        |d| format!("{}0000", d.replace('-', "")),
    );
    let t = to.map_or_else(
        || {
            let now = chrono::Local::now().format("%Y%m%d").to_string();
            format!("{now}2359")
        },
        |d| format!("{}2359", d.replace('-', "")),
    );
    Some(format!("submittedDate:%5B{f}+TO+{t}%5D"))
}
