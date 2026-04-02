mod api;
mod arxiv;
mod display;
mod hf;
mod html;
mod openalex;
mod scholar;
mod text;
mod types;

use anyhow::{bail, Result};
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
}

impl Sort {
    const fn as_api(&self) -> &str {
        match self {
            Self::Hot => "Hot",
            Self::Views => "Views",
            Self::Likes => "Likes",
            Self::Comments => "Comments",
            Self::Github => "GitHub",
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
                serde_json::to_string_pretty(&err).unwrap_or_else(|_| format!("{{\"error\":\"{e:#}\"}}"))
            );
        } else {
            eprintln!("error: {e:#}");
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let use_color =
        !cli.json && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
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
            if cli.json {
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
            if cli.json {
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
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                display::print_batch(&entries, &t);
            }
        }
        Cmd::Read { id } => {
            let clean_id = text::extract_paper_id(&id);
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
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&refs)?);
            } else {
                display::print_scholar_papers(&refs, &t);
            }
        }
        Cmd::Cites { id, limit } => {
            let clean_id = text::extract_paper_id(&id);
            let cites = scholar::fetch_citations(&client.client, &clean_id, limit).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&cites)?);
            } else {
                display::print_scholar_papers(&cites, &t);
            }
        }
    }
    Ok(())
}
