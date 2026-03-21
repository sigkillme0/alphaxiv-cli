use anyhow::{bail, Context, Result};
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use ureq::Agent;

const UA: &str = "alphaxiv-cli/0.4";
const MAX_RETRIES: u32 = 3;

// ── output types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PaperContent {
    pub id: String,
    pub title: String,
    pub sections: Vec<Section>,
}

#[derive(Serialize)]
pub struct Section {
    pub heading: String,
    pub level: u8,
    pub body: String,
}

// ── public api ──────────────────────────────────────────────────────────────

pub fn fetch_paper_content(agent: &Agent, paper_id: &str) -> Result<PaperContent> {
    let url = format!("https://arxiv.org/html/{paper_id}");
    let raw_html = get_html(agent, &url)?;
    parse_paper(&raw_html, paper_id)
}

// ── http fetch with retries ─────────────────────────────────────────────────

fn get_html(agent: &Agent, url: &str) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(500 * (1 << (attempt - 1))));
        }
        match agent.get(url).header("User-Agent", UA).call() {
            Ok(mut resp) => return resp.body_mut().read_to_string().context("reading body"),
            Err(ureq::Error::StatusCode(404)) => {
                bail!("html version not available for this paper")
            }
            Err(ureq::Error::StatusCode(code)) if code != 429 && code < 500 => {
                bail!("http {code}")
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt == MAX_RETRIES {
                    bail!("request failed after retries: {last_err}");
                }
            }
        }
    }
    bail!("request failed: {last_err}")
}

// ── html parsing ────────────────────────────────────────────────────────────

fn parse_paper(raw_html: &str, paper_id: &str) -> Result<PaperContent> {
    let doc = Html::parse_document(raw_html);

    let sel_title = Selector::parse("h1.ltx_title_document").expect("title selector");
    let sel_abstract = Selector::parse("div.ltx_abstract").expect("abstract selector");
    let sel_abstract_p = Selector::parse("p.ltx_p").expect("abstract p selector");
    let sel_section = Selector::parse("section.ltx_section").expect("section selector");
    let sel_subsection = Selector::parse("section.ltx_subsection").expect("subsection selector");
    let sel_subsubsection =
        Selector::parse("section.ltx_subsubsection").expect("subsubsection selector");
    let sel_bib = Selector::parse("section.ltx_bibliography").expect("bibliography selector");
    let sel_bibitem = Selector::parse("li.ltx_bibitem").expect("bibitem selector");

    // extract title
    let title = doc
        .select(&sel_title)
        .next()
        .map(|el| text_without_tags(&el))
        .unwrap_or_default()
        .trim()
        .to_string();

    if title.is_empty() {
        bail!("could not extract title — page may not be a LaTeXML paper");
    }

    let mut sections = Vec::new();

    // extract abstract
    if let Some(abs_el) = doc.select(&sel_abstract).next() {
        let mut body = String::new();
        for p in abs_el.select(&sel_abstract_p) {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(&extract_text(&p));
        }
        if !body.is_empty() {
            sections.push(Section {
                heading: "Abstract".to_string(),
                level: 0,
                body,
            });
        }
    }

    // extract sections
    for sec_el in doc.select(&sel_section) {
        extract_section(
            &sec_el,
            1,
            &sel_subsection,
            &sel_subsubsection,
            &mut sections,
        );
    }

    // extract bibliography
    if let Some(bib_el) = doc.select(&sel_bib).next() {
        let heading_sel = Selector::parse("h2.ltx_title").expect("bib heading selector");
        let heading = bib_el
            .select(&heading_sel)
            .next().map_or_else(|| "References".to_string(), |el| text_without_tags(&el))
            .trim()
            .to_string();

        let mut body = String::new();
        for item in bib_el.select(&sel_bibitem) {
            let text = extract_text(&item).trim().to_string();
            if !text.is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(&text);
            }
        }
        if !body.is_empty() {
            sections.push(Section {
                heading,
                level: 1,
                body,
            });
        }
    }

    Ok(PaperContent {
        id: paper_id.to_string(),
        title,
        sections,
    })
}

// ── section extraction ──────────────────────────────────────────────────────

fn extract_section(
    el: &ElementRef,
    level: u8,
    sel_subsection: &Selector,
    sel_subsubsection: &Selector,
    out: &mut Vec<Section>,
) {
    let heading_tag = match level {
        2 => "h3.ltx_title",
        3 => "h4.ltx_title",
        _ => "h2.ltx_title",
    };
    let heading_sel = Selector::parse(heading_tag).expect("heading selector");

    let heading = el
        .select(&heading_sel)
        .next()
        .map(|h| text_without_tags(&h))
        .unwrap_or_default()
        .trim()
        .to_string();

    // collect body from direct children, skipping subsections and heading
    let body = extract_section_body(el, level);

    if !heading.is_empty() || !body.is_empty() {
        out.push(Section {
            heading,
            level,
            body,
        });
    }

    // recurse into subsections — only direct children (not nested deeper)
    if level == 1 {
        for sub in el.select(sel_subsection) {
            if is_direct_child_section(el, &sub) {
                extract_section(&sub, 2, sel_subsection, sel_subsubsection, out);
            }
        }
    } else if level == 2 {
        for sub in el.select(sel_subsubsection) {
            if is_direct_child_section(el, &sub) {
                extract_section(&sub, 3, sel_subsection, sel_subsubsection, out);
            }
        }
    }
}

/// Check that `child` is a direct section child of `parent` — i.e. there is no
/// intermediate `section.ltx_section` / `section.ltx_subsection` /
/// `section.ltx_subsubsection` between them.
fn is_direct_child_section(parent: &ElementRef, child: &ElementRef) -> bool {
    let parent_html_id = parent.value().attr("id");
    let mut node = child.parent();
    while let Some(p) = node {
        if let Some(el) = ElementRef::wrap(p) {
            let v = el.value();
            // reached the parent element
            if v.attr("id") == parent_html_id && v.name() == parent.value().name() {
                return true;
            }
            // hit an intermediate section → not a direct child
            if v.name() == "section"
                && (v.classes().any(|c| c == "ltx_section")
                    || v.classes().any(|c| c == "ltx_subsection")
                    || v.classes().any(|c| c == "ltx_subsubsection"))
            {
                return false;
            }
        }
        node = p.parent();
    }
    false
}

// ── section body extraction ─────────────────────────────────────────────────

fn extract_section_body(section: &ElementRef, level: u8) -> String {
    let mut parts: Vec<String> = Vec::new();

    // child section class to skip
    let skip_classes: &[&str] = match level {
        1 => &["ltx_subsection"],
        2 => &["ltx_subsubsection"],
        _ => &[],
    };

    for child in section.children() {
        let Some(child_el) = ElementRef::wrap(child) else {
            continue;
        };
        let v = child_el.value();

        // skip child sections
        if v.name() == "section" && skip_classes.iter().any(|c| v.classes().any(|cls| cls == *c)) {
            continue;
        }

        // skip headings (already captured)
        if matches!(v.name(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            continue;
        }

        // div.ltx_para → extract paragraphs
        if v.name() == "div" && v.classes().any(|c| c == "ltx_para") {
            let text = extract_para_text(&child_el);
            if !text.is_empty() {
                parts.push(text);
            }
            continue;
        }

        // figure
        if v.name() == "figure" && v.classes().any(|c| c == "ltx_figure") {
            if let Some(cap) = extract_caption(&child_el, "Figure") {
                parts.push(cap);
            }
            continue;
        }

        // table (figure.ltx_table)
        if v.name() == "figure" && v.classes().any(|c| c == "ltx_table") {
            if let Some(cap) = extract_caption(&child_el, "Table") {
                parts.push(cap);
            }
            continue;
        }

        // display equation at section level
        if v.name() == "table" && v.classes().any(|c| c == "ltx_equation") {
            if let Some(eq) = extract_display_math(&child_el) {
                parts.push(eq);
            }
            continue;
        }

        // some papers wrap content in extra divs; walk one level deeper
        for nested in child_el.children() {
            let Some(n_el) = ElementRef::wrap(nested) else {
                continue;
            };
            let nv = n_el.value();
            if nv.name() == "div" && nv.classes().any(|c| c == "ltx_para") {
                let text = extract_para_text(&n_el);
                if !text.is_empty() {
                    parts.push(text);
                }
            } else if nv.name() == "figure" && nv.classes().any(|c| c == "ltx_figure") {
                if let Some(cap) = extract_caption(&n_el, "Figure") {
                    parts.push(cap);
                }
            } else if nv.name() == "figure" && nv.classes().any(|c| c == "ltx_table") {
                if let Some(cap) = extract_caption(&n_el, "Table") {
                    parts.push(cap);
                }
            } else if nv.name() == "table" && nv.classes().any(|c| c == "ltx_equation") {
                if let Some(eq) = extract_display_math(&n_el) {
                    parts.push(eq);
                }
            }
        }
    }

    parts.join("\n\n")
}

// ── paragraph text extraction ───────────────────────────────────────────────

fn extract_para_text(para: &ElementRef) -> String {
    let sel_p = Selector::parse("p.ltx_p").expect("p selector");
    let sel_equation = Selector::parse("table.ltx_equation").expect("equation selector");
    let mut parts: Vec<String> = Vec::new();

    for child in para.children() {
        let Some(child_el) = ElementRef::wrap(child) else {
            continue;
        };
        let v = child_el.value();

        if v.name() == "p" && v.classes().any(|c| c == "ltx_p") {
            let text = extract_text(&child_el).trim().to_string();
            if !text.is_empty() {
                parts.push(text);
            }
        } else if v.name() == "table" && v.classes().any(|c| c == "ltx_equation") {
            if let Some(eq) = extract_display_math(&child_el) {
                parts.push(eq);
            }
        }
    }

    // fallback: use select to find nested p.ltx_p / equations
    if parts.is_empty() {
        for p in para.select(&sel_p) {
            let text = extract_text(&p).trim().to_string();
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    if parts.is_empty() {
        for eq in para.select(&sel_equation) {
            if let Some(e) = extract_display_math(&eq) {
                parts.push(e);
            }
        }
    }

    parts.join("\n\n")
}

// ── inline text extraction (handling math) ──────────────────────────────────

fn extract_text(el: &ElementRef) -> String {
    let mut out = String::new();
    extract_text_inner(el, &mut out);
    out
}

fn extract_text_inner(el: &ElementRef, out: &mut String) {
    for child in el.children() {
        match child.value() {
            scraper::Node::Text(t) => {
                out.push_str(t);
            }
            scraper::Node::Element(_) => {
                let Some(child_el) = ElementRef::wrap(child) else {
                    continue;
                };
                let v = child_el.value();

                // skip section-number tags
                if v.name() == "span" && v.classes().any(|c| c == "ltx_tag") {
                    continue;
                }

                // math element → extract alttext
                if v.name() == "math" {
                    if let Some(alt) = v.attr("alttext") {
                        let is_block = v.attr("display") == Some("block");
                        if is_block {
                            out.push_str("$$");
                            out.push_str(alt);
                            out.push_str("$$");
                        } else {
                            out.push('$');
                            out.push_str(alt);
                            out.push('$');
                        }
                    }
                    continue;
                }

                // skip nav, header, footer
                if v.name() == "nav" && v.classes().any(|c| c == "ltx_page_navbar") {
                    continue;
                }
                if v.classes().any(|c| c == "ltx_page_header" || c == "ltx_page_footer") {
                    continue;
                }

                extract_text_inner(&child_el, out);
            }
            _ => {}
        }
    }
}

// ── display math extraction ─────────────────────────────────────────────────

fn extract_display_math(eq: &ElementRef) -> Option<String> {
    let sel_math = Selector::parse("math.ltx_Math").expect("math selector");
    for m in eq.select(&sel_math) {
        if let Some(alt) = m.value().attr("alttext") {
            if !alt.is_empty() {
                return Some(format!("$${alt}$$"));
            }
        }
    }
    None
}

// ── caption extraction ──────────────────────────────────────────────────────

fn extract_caption(fig: &ElementRef, kind: &str) -> Option<String> {
    let sel_caption = Selector::parse("figcaption.ltx_caption").expect("caption selector");
    let sel_tag = Selector::parse("span.ltx_tag").expect("caption tag selector");
    let cap_el = fig.select(&sel_caption).next()?;

    let tag_text = cap_el
        .select(&sel_tag)
        .next()
        .map(|t| t.text().collect::<String>())
        .unwrap_or_default();
    let full_text = extract_text(&cap_el).trim().to_string();

    if !tag_text.is_empty() {
        // strip the tag from the caption text to get just the description
        let desc = full_text
            .strip_prefix(tag_text.trim())
            .unwrap_or(&full_text)
            .trim_start_matches(':')
            .trim_start_matches('.')
            .trim();
        if desc.is_empty() {
            Some(format!("[{}]", tag_text.trim()))
        } else {
            Some(format!("[{}: {}]", tag_text.trim(), desc))
        }
    } else if !full_text.is_empty() {
        Some(format!("[{kind}: {full_text}]"))
    } else {
        None
    }
}

// ── text extraction skipping tag spans ──────────────────────────────────────

/// Extract text from an element, skipping `span.ltx_tag` children (section numbers).
fn text_without_tags(el: &ElementRef) -> String {
    let mut out = String::new();
    collect_text_skip_tags(el, &mut out);
    normalize_ws(&out)
}

fn collect_text_skip_tags(el: &ElementRef, out: &mut String) {
    for child in el.children() {
        match child.value() {
            scraper::Node::Text(t) => out.push_str(t),
            scraper::Node::Element(_) => {
                let Some(child_el) = ElementRef::wrap(child) else {
                    continue;
                };
                let v = child_el.value();
                // skip tag spans (section numbers like "1 ", "2.1 ")
                if v.name() == "span" && v.classes().any(|c| c == "ltx_tag") {
                    continue;
                }
                // handle math in titles
                if v.name() == "math" {
                    if let Some(alt) = v.attr("alttext") {
                        out.push('$');
                        out.push_str(alt);
                        out.push('$');
                        continue;
                    }
                }
                collect_text_skip_tags(&child_el, out);
            }
            _ => {}
        }
    }
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
