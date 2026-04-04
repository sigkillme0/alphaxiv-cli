mod api;
mod arxiv;
mod display;
mod hf;
mod html;
mod openalex;
mod retry;
mod scholar;
mod text;
mod types;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::io::IsTerminal;

// ── cli ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "alphaxiv", version, about = "look up arxiv papers from the terminal")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// get json output instead of pretty-printed text
    #[arg(long, global = true)]
    json: bool,
    /// just print paper ids, one per line (for piping)
    #[arg(long, global = true)]
    ids: bool,
    /// keep markdown/html formatting instead of stripping it
    #[arg(long, global = true)]
    raw: bool,
}

#[derive(Clone, ValueEnum)]
enum Sort {
    Hot,
    Views,
    Likes,
    Comments,
    Github,
    #[value(name = "twitter")]
    Twitter,
}

impl Sort {
    const fn as_api(&self) -> &str {
        match self {
            Self::Hot => "Hot",
            Self::Views => "Views",
            Self::Likes => "Likes",
            Self::Comments => "Comments",
            Self::Github => "GitHub",
            Self::Twitter => "Twitter (X)",
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Interval {
    #[value(name = "3d")]
    ThreeDays,
    #[value(name = "7d")]
    SevenDays,
    #[value(name = "30d")]
    ThirtyDays,
    #[value(name = "90d")]
    NinetyDays,
    #[value(name = "all")]
    AllTime,
}

impl Interval {
    const fn as_api(&self) -> &str {
        match self {
            Self::ThreeDays => "3 Days",
            Self::SevenDays => "7 Days",
            Self::ThirtyDays => "30 Days",
            Self::NinetyDays => "90 Days",
            Self::AllTime => "All time",
        }
    }
}

#[derive(Clone, ValueEnum)]
enum SearchSort {
    Relevance,
    Submitted,
    Updated,
}

impl SearchSort {
    const fn as_api(&self) -> &str {
        match self {
            Self::Relevance => "relevance",
            Self::Submitted => "submittedDate",
            Self::Updated => "lastUpdatedDate",
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// see what's trending on arxiv
    Feed {
        /// how many papers
        #[arg(short, long, default_value = "25")]
        limit: usize,
        /// which page (starts at 0)
        #[arg(short, long, default_value = "0")]
        page: usize,
        /// sort by
        #[arg(short, long, default_value = "hot")]
        sort: Sort,
        /// time window
        #[arg(short, long, default_value = "7d")]
        interval: Interval,
    },
    /// look up a paper — pass an arxiv id, url, or doi
    Paper {
        /// e.g. 2502.11089 or <https://arxiv.org/abs/2502.11089>
        id: String,
        /// also grab the alphaxiv overview/blog post
        #[arg(long)]
        overview: bool,
        /// just print the bibtex and exit
        #[arg(long)]
        bibtex: bool,
        /// don't fetch comments
        #[arg(long)]
        no_comments: bool,
    },
    /// search for papers
    Search {
        /// what to search for
        query: Vec<String>,
        /// cap the number of results
        #[arg(short, long)]
        limit: Option<usize>,
        /// sort by (submitted/updated use arxiv api directly)
        #[arg(long, default_value = "relevance")]
        sort: SearchSort,
        /// sort ascending instead of descending
        #[arg(long)]
        asc: bool,
        /// filter by submission date (start, YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// filter by submission date (end, YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
    },
    /// look up several papers at once (runs in parallel)
    Batch {
        /// arxiv ids or urls
        ids: Vec<String>,
        /// also grab overviews
        #[arg(long)]
        overview: bool,
        /// don't fetch comments
        #[arg(long)]
        no_comments: bool,
    },
    /// read the full text of a paper (from arxiv html)
    Read {
        /// arxiv id or url
        id: String,
    },
    /// what papers does this one cite?
    Refs {
        /// arxiv id or url
        id: String,
    },
    /// what papers cite this one?
    Cites {
        /// arxiv id or url
        id: String,
        /// how many to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// browse latest papers in an arxiv category
    New {
        /// arxiv category (e.g. cs.AI, hep-th, math.CO)
        category: String,
        /// how many papers
        #[arg(short, long, default_value = "25")]
        limit: usize,
        /// which page (starts at 0)
        #[arg(short, long, default_value = "0")]
        page: usize,
        /// start date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// end date (YYYY-MM-DD)
        #[arg(long)]
        to: Option<String>,
    },
    /// look up an author by name
    Author {
        /// author name, e.g. "Geoffrey Hinton"
        name: Vec<String>,
        /// how many top papers to show
        #[arg(short, long, default_value = "5")]
        limit: usize,
    },
    /// find similar/recommended papers
    Similar {
        /// arxiv id or url
        id: String,
        /// how many to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// find related papers via openalex
    Related {
        /// arxiv id or url
        id: String,
        /// how many to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// download a paper's pdf
    Download {
        /// arxiv id or url
        id: String,
        /// output filename (default: {id}.pdf)
        #[arg(short, long)]
        output: Option<String>,
    },
}

// ── main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json_mode = cli.json;
    if let Err(e) = run(cli).await {
        if json_mode {
            let err = serde_json::json!({ "error": format!("{:#}", e) });
            println!(
                "{}",
                serde_json::to_string_pretty(&err).expect("json Value is always serializable")
            );
        } else {
            eprintln!("error: {e:#}");
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let use_color =
        !cli.json && !cli.ids && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let t = display::Theme::new(use_color);
    let client = api::ApiClient::new()?;
    let raw = cli.raw;

    match cli.cmd {
        Cmd::Feed {
            limit,
            page,
            sort,
            interval,
        } => {
            if limit == 0 {
                bail!("limit must be greater than 0");
            }
            let entries = client.fetch_feed(page, limit, sort.as_api(), interval.as_api()).await?;
            if entries.is_empty() {
                if cli.json {
                    println!("[]");
                } else {
                    eprintln!("no papers found");
                }
                return Ok(());
            }
            if cli.ids {
                for e in &entries {
                    println!("{}", e.id);
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                display::print_feed(&entries, page * limit, &t);
            }
        }
        Cmd::Paper {
            id,
            overview,
            bibtex,
            no_comments,
        } => {
            let paper = client.fetch_paper(
                &id,
                overview && !bibtex,
                !bibtex && !no_comments,
                raw,
            ).await?;

            if cli.ids {
                println!("{}", paper.id);
                return Ok(());
            }

            if bibtex {
                match paper.bibtex {
                    Some(ref b) => {
                        if cli.json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(
                                    &serde_json::json!({ "id": paper.id, "bibtex": b.trim() })
                                )?
                            );
                        } else {
                            println!("{}", b.trim());
                        }
                        return Ok(());
                    }
                    None => bail!("no bibtex available for this paper"),
                }
            }

            if overview && paper.overview.is_none() {
                eprintln!(
                    "{}",
                    t.warn.style("note: no overview available for this paper")
                );
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&paper)?);
            } else {
                display::print_paper(&paper, &t);
            }
        }
        Cmd::Search { query, limit, sort, asc, from, to } => {
            let q = query.join(" ");
            if q.is_empty() {
                bail!("search query required");
            }
            let hits = if matches!(sort, SearchSort::Relevance) && from.is_none() && to.is_none() {
                client.search_papers(&q, limit).await?
            } else {
                let order = if asc { "ascending" } else { "descending" };
                let max = limit.unwrap_or(25);
                arxiv::search(
                    &client.client,
                    &q,
                    sort.as_api(),
                    order,
                    0,
                    max,
                    from.as_deref(),
                    to.as_deref(),
                ).await?
            };
            if cli.ids {
                for h in &hits {
                    println!("{}", h.id);
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else {
                display::print_search(&hits, &t);
            }
        }
        Cmd::Batch {
            ids,
            overview,
            no_comments,
        } => {
            if ids.is_empty() {
                bail!("at least one paper id required");
            }
            let entries = client.fetch_batch(&ids, overview, !no_comments, raw).await;
            if cli.ids {
                for entry in &entries {
                    println!("{}", entry.id);
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                display::print_batch(&entries, &t);
            }
        }
        Cmd::Read { id } => {
            let clean_id = text::extract_paper_id(&id);
            if cli.ids {
                println!("{clean_id}");
                return Ok(());
            }
            let content = html::fetch_paper_content(&client.client, &clean_id).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&content)?);
            } else {
                display::print_paper_content(&content, &t);
            }
        }
        Cmd::Refs { id } => {
            let clean_id = text::extract_paper_id(&id);
            let refs = scholar::fetch_references(&client.client, &clean_id).await?;
            if cli.ids {
                for r in &refs {
                    if let Some(ref arxiv_id) = r.arxiv_id {
                        println!("{arxiv_id}");
                    }
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&refs)?);
            } else {
                display::print_scholar_papers(&refs, &t);
            }
        }
        Cmd::Cites { id, limit } => {
            let clean_id = text::extract_paper_id(&id);
            let cites = scholar::fetch_citations(&client.client, &clean_id, limit).await?;
            if cli.ids {
                for c in &cites {
                    if let Some(ref arxiv_id) = c.arxiv_id {
                        println!("{arxiv_id}");
                    }
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&cites)?);
            } else {
                display::print_scholar_papers(&cites, &t);
            }
        }
        Cmd::New {
            category,
            limit,
            page,
            from,
            to,
        } => {
            if limit == 0 {
                bail!("limit must be greater than 0");
            }
            let start = page * limit;
            let hits = arxiv::browse_category(
                &client.client,
                &category,
                start,
                limit,
                from.as_deref(),
                to.as_deref(),
            ).await?;
            if hits.is_empty() {
                if cli.json {
                    println!("[]");
                } else {
                    eprintln!("no papers found");
                }
                return Ok(());
            }
            if cli.ids {
                for h in &hits {
                    println!("{}", h.id);
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else {
                display::print_search(&hits, &t);
            }
        }
        Cmd::Author { name, limit } => {
            let q = name.join(" ");
            if q.is_empty() {
                bail!("author name required");
            }
            let mut author = scholar::search_author(&client.client, &q).await?;
            let papers =
                scholar::fetch_author_papers(&client.client, &author.id, limit).await?;
            author.papers = papers;
            if cli.ids {
                for p in &author.papers {
                    if let Some(ref arxiv_id) = p.arxiv_id {
                        println!("{arxiv_id}");
                    }
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&author)?);
            } else {
                display::print_author(&author, &t);
            }
        }
        Cmd::Similar { id, limit } => {
            let clean_id = text::extract_paper_id(&id);
            let similar = scholar::fetch_similar(&client.client, &clean_id, limit).await?;
            if cli.ids {
                for p in &similar {
                    if let Some(ref arxiv_id) = p.arxiv_id {
                        println!("{arxiv_id}");
                    }
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&similar)?);
            } else {
                display::print_scholar_papers(&similar, &t);
            }
        }
        Cmd::Related { id, limit } => {
            let clean_id = text::extract_paper_id(&id);
            let related = openalex::fetch_related(&client.client, &clean_id, limit).await?;
            if cli.ids {
                for p in &related {
                    if let Some(ref arxiv_id) = p.arxiv_id {
                        println!("{arxiv_id}");
                    }
                }
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&related)?);
            } else {
                display::print_scholar_papers(&related, &t);
            }
        }
        Cmd::Download { id, output } => {
            let clean_id = text::extract_paper_id(&id);
            let url = format!("https://arxiv.org/pdf/{clean_id}");
            eprintln!("downloading {url}");
            let resp = client
                .client
                .get(&url)
                .send()
                .await
                .context("failed to request pdf")?;
            let status = resp.status().as_u16();
            if status == 404 {
                bail!("paper not found: {clean_id}");
            }
            if !(200..300).contains(&status) {
                bail!("download failed: http {status}");
            }
            let bytes = resp.bytes().await.context("failed to read pdf bytes")?;
            let filename = output.unwrap_or_else(|| {
                format!("{}.pdf", clean_id.replace('/', "_"))
            });
            std::fs::write(&filename, &bytes)
                .with_context(|| format!("failed to write {filename}"))?;
            let size = bytes.len();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "file": filename,
                        "size": size,
                        "id": clean_id,
                    }))?
                );
            } else {
                eprintln!("saved {} ({})", filename, format_size(size));
            }
        }
    }
    Ok(())
}

fn format_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
