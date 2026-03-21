use crate::types::{FeedEntry, PaperOut, SearchOut, BatchEntry};
use chrono::{Datelike, Local, NaiveDate};
use owo_colors::Style;

pub struct Theme {
    pub title: Style,
    pub dim: Style,
    pub accent: Style,
    pub warn: Style,
    pub heading: Style,
    pub idx: Style,
}

impl Theme {
    pub fn new(color: bool) -> Self {
        if color {
            Self {
                title: Style::new().bold(),
                dim: Style::new().dimmed(),
                accent: Style::new().cyan(),
                warn: Style::new().yellow(),
                heading: Style::new().magenta().bold(),
                idx: Style::new().yellow().bold(),
            }
        } else {
            Self {
                title: Style::new(),
                dim: Style::new(),
                accent: Style::new(),
                warn: Style::new(),
                heading: Style::new(),
                idx: Style::new(),
            }
        }
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn make_byline(authors: &[String], org: Option<&str>, max: usize) -> String {
    let mut parts = Vec::new();
    if !authors.is_empty() {
        if authors.len() <= max {
            parts.push(authors.join(", "));
        } else {
            let shown: Vec<&str> = authors[..max].iter().map(std::string::String::as_str).collect();
            parts.push(format!(
                "{} +{} more",
                shown.join(", "),
                authors.len() - max
            ));
        }
    }
    if let Some(o) = org {
        parts.push(format!("({o})"));
    }
    parts.join(" ")
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn human_date(iso: &str) -> String {
    let Ok(date) = NaiveDate::parse_from_str(iso, "%Y-%m-%d") else {
        return iso.to_string();
    };
    let today = Local::now().date_naive();
    let days = (today - date).num_days();
    if days == 0 {
        return "today".to_string();
    }
    if days == 1 {
        return "yesterday".to_string();
    }
    if (2..=6).contains(&days) {
        return format!("{days}d ago");
    }
    let m = MONTHS[(date.month() - 1) as usize];
    if date.year() == today.year() {
        format!("{} {}", m, date.day())
    } else {
        format!("{} {}, {}", m, date.day(), date.year())
    }
}

fn fmt_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn normalize_paragraphs(s: &str) -> String {
    s.split("\n\n")
        .map(|para| para.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|para| !para.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ── feed ────────────────────────────────────────────────────────────────────

pub fn print_feed(entries: &[FeedEntry], offset: usize, t: &Theme) {
    for (i, e) in entries.iter().enumerate() {
        print!("{} ", t.idx.style(format!("[{}]", offset + i + 1)));
        println!("{}", t.title.style(&e.title));

        // author line: first author + org, or just org, keep it short
        let byline = make_byline(&e.authors, e.organization.as_deref(), 2);
        if !byline.is_empty() {
            println!("    {}", t.dim.style(&byline));
        }

        // single metadata line: date, stats, arxiv categories
        let mut meta = Vec::new();
        if let Some(ref d) = e.date {
            meta.push(human_date(d));
        }
        meta.push(format!("{} views", fmt_count(e.views)));
        meta.push(format!("{} likes", fmt_count(e.likes)));
        let cats: Vec<&str> = e
            .topics
            .iter()
            .filter(|s| s.contains('.'))
            .map(std::string::String::as_str)
            .collect();
        if !cats.is_empty() {
            meta.push(cats.join(", "));
        }
        println!("    {}", t.dim.style(meta.join("  ")));
        println!(
            "    {}",
            t.accent.style(format!("{}/abs/{}", crate::api::SITE, e.id))
        );
        if let Some(ref s) = e.summary {
            println!("    {}", t.dim.style(s));
        }
        println!();
    }
}

// ── paper ───────────────────────────────────────────────────────────────────

pub fn print_paper(p: &PaperOut, t: &Theme) {
    println!("{}", t.title.style(&p.title));
    if !p.authors.is_empty() {
        let org = if p.organizations.is_empty() {
            None
        } else {
            Some(p.organizations.join(", "))
        };
        let byline = make_byline(&p.authors, org.as_deref(), 5);
        println!("{}", t.dim.style(&byline));
    } else if !p.organizations.is_empty() {
        println!("{}", t.dim.style(p.organizations.join(", ")));
    }
    let mut meta = Vec::new();
    if let Some(ref d) = p.date {
        meta.push(human_date(d));
    }
    if let Some(ref v) = p.version {
        meta.push(v.clone());
    }
    if let Some(ref venue) = p.venue {
        meta.push(venue.clone());
    }
    if !meta.is_empty() {
        println!("{}", t.dim.style(meta.join("  ")));
    }
    let stats = if p.comments.is_empty() {
        format!(
            "{} views  {} likes  {} comments",
            fmt_count(p.views),
            fmt_count(p.likes),
            p.comment_count
        )
    } else {
        let total = p.comments.len() + p.reply_count;
        if p.reply_count > 0 {
            format!(
                "{} views  {} likes  {} comments ({} threads, {} replies)",
                fmt_count(p.views),
                fmt_count(p.likes),
                total,
                p.comments.len(),
                p.reply_count
            )
        } else {
            format!(
                "{} views  {} likes  {} comments",
                fmt_count(p.views),
                fmt_count(p.likes),
                total
            )
        }
    };
    println!("{}", t.dim.style(stats));
    if let Some(citations) = p.citation_count {
        let mut cite_parts = vec![format!("{} citations", fmt_count(citations))];
        if let Some(inf) = p.influential_citation_count {
            if inf > 0 {
                cite_parts.push(format!("{} influential", fmt_count(inf)));
            }
        }
        if let Some(refs) = p.reference_count {
            cite_parts.push(format!("{refs} references"));
        }
        println!("{}", t.dim.style(cite_parts.join("  ")));
    }
    if let Some(ref tldr) = p.tldr {
        println!("\n{} {}", t.accent.style("tldr:"), tldr);
    }
    println!();

    if !p.abstract_text.is_empty() {
        println!("{}\n", normalize_paragraphs(&p.abstract_text));
    }

    if !p.topics.is_empty() {
        println!(
            "{} {}\n",
            t.accent.style("topics:"),
            t.dim.style(p.topics.join(", "))
        );
    }

    if let Some(ref gh) = p.github {
        let mut parts = vec![gh.url.clone()];
        if let Some(stars) = gh.stars {
            parts.push(format!("{} stars", fmt_count(stars)));
        }
        if let Some(ref lang) = gh.language {
            parts.push(lang.clone());
        }
        println!("{} {}\n", t.accent.style("github:"), parts.join("  "));
    }

    if let Some(ref ov) = p.overview {
        println!("{}\n", t.heading.style("--- overview ---"));
        println!("{ov}\n");
    }

    if !p.comments.is_empty() {
        println!("{}", t.heading.style("--- comments ---"));
        for c in &p.comments {
            let votes = if c.upvotes > 0 {
                format!(" [+{}]", c.upvotes)
            } else {
                String::new()
            };
            let reply_hint = if c.replies.is_empty() {
                String::new()
            } else {
                let n = c.replies.len();
                format!(" ({} {})", n, if n == 1 { "reply" } else { "replies" })
            };
            let date_hint = c
                .date
                .as_deref()
                .map(|d| format!(" ({})", human_date(d)))
                .unwrap_or_default();
            println!(
                "  {} {}{}{}{}",
                t.accent.style(">"),
                t.title.style(&c.author),
                t.dim.style(&date_hint),
                t.dim.style(&votes),
                t.dim.style(&reply_hint)
            );
            if let Some(ref title) = c.title {
                println!("    {}", t.title.style(title));
            }
            if let Some(ref ctx) = c.context {
                println!("    {}", t.dim.style(format!("\"{ctx}\"")));
            }
            println!("    {}\n", c.text);
            for r in &c.replies {
                let rv = if r.upvotes > 0 {
                    format!(" [+{}]", r.upvotes)
                } else {
                    String::new()
                };
                let rd = r
                    .date
                    .as_deref()
                    .map(|d| format!(" ({})", human_date(d)))
                    .unwrap_or_default();
                println!(
                    "      {} {}{}{}",
                    t.accent.style(">>"),
                    t.title.style(&r.author),
                    t.dim.style(&rd),
                    t.dim.style(&rv)
                );
                println!("        {}\n", r.text);
            }
        }
    }

    if let Some(ref bib) = p.bibtex {
        println!("{}\n{}\n", t.heading.style("--- bibtex ---"), bib.trim());
    }

    println!("{} {}", t.accent.style("alphaxiv:"), p.alphaxiv_url);
    println!("{} {}", t.accent.style("arxiv:   "), p.arxiv_url);
    if let Some(ref pdf) = p.pdf_url {
        println!("{} {}", t.accent.style("pdf:     "), pdf);
    }
}

// ── search ──────────────────────────────────────────────────────────────────

pub fn print_search(hits: &[SearchOut], t: &Theme) {
    if hits.is_empty() {
        eprintln!("no results");
        return;
    }
    for (i, h) in hits.iter().enumerate() {
        print!("{} ", t.idx.style(format!("[{}]", i + 1)));
        println!("{}", t.title.style(&h.title));
        let byline = make_byline(&h.authors, None, 3);
        if !byline.is_empty() {
            println!("    {}", t.dim.style(&byline));
        }
        let mut meta = Vec::new();
        if let Some(ref d) = h.date {
            meta.push(human_date(d));
        }
        meta.push(h.id.clone());
        println!("    {}", t.dim.style(meta.join("  ")));
        println!("    {}", t.accent.style(&h.url));
        if let Some(ref abs) = h.abstract_text {
            println!("    {}", t.dim.style(normalize_paragraphs(abs)));
        }
        println!();
    }
}

// ── batch ───────────────────────────────────────────────────────────────────

pub fn print_batch(entries: &[BatchEntry], t: &Theme) {
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            println!(
                "{}",
                t.dim.style("────────────────────────────────────────")
            );
        }
        if let Some(ref paper) = entry.paper {
            print_paper(paper, t);
        } else if let Some(ref err) = entry.error {
            println!(
                "{} {}: {}",
                t.warn.style("error:"),
                t.title.style(&entry.id),
                err
            );
        }
        println!();
    }
}

// ── read (paper content) ────────────────────────────────────────────────────

pub fn print_paper_content(content: &crate::html::PaperContent, t: &Theme) {
    println!("{}\n", t.title.style(&content.title));
    for sec in &content.sections {
        let prefix = match sec.level {
            0 => "",
            1 => "# ",
            2 => "## ",
            _ => "### ",
        };
        println!("{}", t.heading.style(format!("{}{}", prefix, sec.heading)));
        println!();
        println!("{}", sec.body);
        println!();
    }
}

// ── refs / cites ────────────────────────────────────────────────────────────

pub fn print_scholar_papers(papers: &[crate::scholar::ScholarPaper], t: &Theme) {
    if papers.is_empty() {
        eprintln!("none found");
        return;
    }
    for (i, p) in papers.iter().enumerate() {
        print!("{} ", t.idx.style(format!("[{}]", i + 1)));
        println!("{}", t.title.style(&p.title));
        let byline = make_byline(&p.authors, None, 3);
        if !byline.is_empty() {
            println!("    {}", t.dim.style(&byline));
        }
        let mut meta = Vec::new();
        if let Some(y) = p.year {
            meta.push(y.to_string());
        }
        if let Some(c) = p.citation_count {
            meta.push(format!("{} citations", fmt_count(c)));
        }
        if let Some(ref aid) = p.arxiv_id {
            meta.push(aid.clone());
        }
        if !meta.is_empty() {
            println!("    {}", t.dim.style(meta.join("  ")));
        }
        println!();
    }
}
