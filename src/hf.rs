use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

const HF_API: &str = "https://huggingface.co/api";
pub const HF_SITE: &str = "https://huggingface.co";
const LIMIT: usize = 10;

// ── api response types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PaperResp {
    upvotes: Option<u32>,
    #[serde(rename = "githubRepo")]
    github_repo: Option<String>,
    #[serde(rename = "githubStars")]
    github_stars: Option<u64>,
}

#[derive(Deserialize)]
struct ModelResp {
    id: String,
    #[serde(default)]
    likes: u64,
    #[serde(default)]
    downloads: u64,
    pipeline_tag: Option<String>,
}

#[derive(Deserialize)]
struct DatasetResp {
    id: String,
    #[serde(default)]
    likes: u64,
    #[serde(default)]
    downloads: u64,
}

#[derive(Deserialize)]
struct SpaceResp {
    id: String,
    #[serde(default)]
    likes: u64,
}

// ── enrichment output ───────────────────────────────────────────────────────

pub struct HfEnrichment {
    pub upvotes: Option<u32>,
    pub github_url: Option<String>,
    pub github_stars: Option<u64>,
    pub models: Vec<HfModel>,
    pub datasets: Vec<HfDataset>,
    pub spaces: Vec<HfSpace>,
}

pub struct HfModel {
    pub id: String,
    pub likes: u64,
    pub downloads: u64,
    pub pipeline: Option<String>,
}

pub struct HfDataset {
    pub id: String,
    pub likes: u64,
    pub downloads: u64,
}

pub struct HfSpace {
    pub id: String,
    pub likes: u64,
}

// ── http ────────────────────────────────────────────────────────────────────

async fn hf_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let body = client
        .get(url)
        .send()
        .await
        .context("huggingface request")?
        .text()
        .await
        .context("reading hf body")?;
    serde_json::from_str(&body).context("parsing hf response")
}

// ── public api ──────────────────────────────────────────────────────────────

pub async fn fetch_hf_enrichment(client: &Client, paper_id: &str) -> Result<HfEnrichment> {
    let (paper, models_raw, datasets_raw, spaces_raw) = tokio::join!(
        async { hf_json::<PaperResp>(client, &format!("{HF_API}/papers/{paper_id}")).await.ok() },
        async {
            hf_json::<Vec<ModelResp>>(
                client,
                &format!(
                    "{HF_API}/models?filter=arxiv:{paper_id}&sort=likes&direction=-1&limit={LIMIT}"
                ),
            )
            .await
            .unwrap_or_default()
        },
        async {
            hf_json::<Vec<DatasetResp>>(
                client,
                &format!(
                    "{HF_API}/datasets?filter=arxiv:{paper_id}&sort=likes&direction=-1&limit={LIMIT}"
                ),
            )
            .await
            .unwrap_or_default()
        },
        async {
            hf_json::<Vec<SpaceResp>>(
                client,
                &format!(
                    "{HF_API}/spaces?filter=arxiv:{paper_id}&sort=likes&direction=-1&limit={LIMIT}"
                ),
            )
            .await
            .unwrap_or_default()
        },
    );

    let (upvotes, github_url, github_stars) = match paper {
        Some(p) => (p.upvotes, p.github_repo, p.github_stars),
        None => (None, None, None),
    };

    Ok(HfEnrichment {
        upvotes,
        github_url,
        github_stars,
        models: models_raw
            .into_iter()
            .map(|m| HfModel {
                id: m.id,
                likes: m.likes,
                downloads: m.downloads,
                pipeline: m.pipeline_tag,
            })
            .collect(),
        datasets: datasets_raw
            .into_iter()
            .map(|d| HfDataset {
                id: d.id,
                likes: d.likes,
                downloads: d.downloads,
            })
            .collect(),
        spaces: spaces_raw
            .into_iter()
            .map(|sp| HfSpace {
                id: sp.id,
                likes: sp.likes,
            })
            .collect(),
    })
}
