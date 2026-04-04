// ── url encoding ────────────────────────────────────────────────────────────

pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0xf) as usize] as char);
            }
        }
    }
    out
}

// ── date formatting ─────────────────────────────────────────────────────────

pub fn format_date(raw: &str) -> String {
    // rfc3339: "2024-01-15T00:00:00Z" or "2024-01-15T00:00:00+00:00"
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return dt.format("%Y-%m-%d").to_string();
    }
    // iso8601 without timezone: "2024-01-15T00:00:00"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S") {
        return dt.format("%Y-%m-%d").to_string();
    }
    // plain date: "2024-01-15"
    if raw.len() == 10 && raw.as_bytes().get(4) == Some(&b'-') && raw.as_bytes().get(7) == Some(&b'-')
        && raw.parse::<chrono::NaiveDate>().is_ok() {
            return raw.to_string();
        }
    // JS Date.toString(): "Tue Jun 13 2017 00:57:34 GMT+0000 (...)"
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() >= 4 {
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        if let Some(mi) = months
            .iter()
            .position(|&m| m.eq_ignore_ascii_case(parts[1]))
        {
            if let (Ok(day), Ok(year)) = (parts[2].parse::<u32>(), parts[3].parse::<u32>()) {
                return format!("{}-{:02}-{:02}", year, mi + 1, day);
            }
        }
    }
    raw.to_string()
}

// ── paper id extraction ─────────────────────────────────────────────────────

pub fn extract_paper_id(raw: &str) -> String {
    let s = raw.trim();
    let url_prefixes = [
        "https://arxiv.org/abs/",
        "http://arxiv.org/abs/",
        "https://www.arxiv.org/abs/",
        "http://www.arxiv.org/abs/",
        "https://arxiv.org/pdf/",
        "http://arxiv.org/pdf/",
        "https://www.arxiv.org/pdf/",
        "http://www.arxiv.org/pdf/",
        "https://arxiv.org/html/",
        "http://arxiv.org/html/",
        "https://arxiv.org/e-print/",
        "http://arxiv.org/e-print/",
        "https://alphaxiv.org/abs/",
        "https://www.alphaxiv.org/abs/",
        "http://alphaxiv.org/abs/",
        "http://www.alphaxiv.org/abs/",
        "https://alphaxiv.org/pdf/",
        "https://www.alphaxiv.org/pdf/",
        "http://alphaxiv.org/pdf/",
    ];
    for prefix in &url_prefixes {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.trim_end_matches(".pdf").trim_end_matches('/').to_string();
        }
    }
    // DOI format: https://doi.org/10.48550/arXiv.2502.11089
    for doi_prefix in &[
        "https://doi.org/10.48550/arXiv.",
        "http://doi.org/10.48550/arXiv.",
        "https://doi.org/10.48550/arxiv.",
        "http://doi.org/10.48550/arxiv.",
    ] {
        if let Some(rest) = s.strip_prefix(doi_prefix) {
            return rest.to_string();
        }
    }
    // citation format: arXiv:2502.11089 or arxiv:2502.11089
    if let Some(rest) = s.strip_prefix("arXiv:").or_else(|| s.strip_prefix("arxiv:")) {
        return rest.to_string();
    }
    s.to_string()
}

// ── html entity decoding ────────────────────────────────────────────────────

pub fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        if let Some(semi) = rest[1..].find(';') {
            let entity = &rest[1..=semi];
            let decoded = match entity {
                "amp" => Some("&"),
                "lt" => Some("<"),
                "gt" => Some(">"),
                "quot" => Some("\""),
                "apos" => Some("'"),
                "nbsp" => Some(" "),
                "mdash" | "#8212" => Some("\u{2014}"),
                "ndash" | "#8211" => Some("\u{2013}"),
                "hellip" | "#8230" => Some("\u{2026}"),
                "laquo" | "#171" => Some("\u{00AB}"),
                "raquo" | "#187" => Some("\u{00BB}"),
                "ldquo" | "#8220" => Some("\u{201C}"),
                "rdquo" | "#8221" => Some("\u{201D}"),
                "lsquo" | "#8216" => Some("\u{2018}"),
                "rsquo" | "#8217" => Some("\u{2019}"),
                "times" | "#215" => Some("\u{00D7}"),
                "divide" | "#247" => Some("\u{00F7}"),
                "plusmn" | "#177" => Some("\u{00B1}"),
                "infin" | "#8734" => Some("\u{221E}"),
                "ne" | "#8800" => Some("\u{2260}"),
                "le" | "#8804" => Some("\u{2264}"),
                "ge" | "#8805" => Some("\u{2265}"),
                "alpha" | "#945" => Some("\u{03B1}"),
                "beta" | "#946" => Some("\u{03B2}"),
                "gamma" | "#947" => Some("\u{03B3}"),
                "delta" | "#948" => Some("\u{03B4}"),
                "pi" | "#960" => Some("\u{03C0}"),
                "sigma" | "#963" => Some("\u{03C3}"),
                "theta" | "#952" => Some("\u{03B8}"),
                "lambda" | "#955" => Some("\u{03BB}"),
                "mu" | "#956" => Some("\u{03BC}"),
                _ => None,
            };
            if let Some(replacement) = decoded {
                out.push_str(replacement);
                rest = &rest[semi + 2..];
                continue;
            }
            // numeric entities: &#123; or &#x7B;
            if let Some(num_part) = entity.strip_prefix('#') {
                let code = num_part
                    .strip_prefix('x')
                    .or_else(|| num_part.strip_prefix('X'))
                    .map_or_else(
                        || num_part.parse::<u32>().ok(),
                        |hex| u32::from_str_radix(hex, 16).ok(),
                    );
                if let Some(c) = code.and_then(char::from_u32) {
                    out.push(c);
                    rest = &rest[semi + 2..];
                    continue;
                }
            }
        }
        // not a recognized entity — pass through the &
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

// ── markdown processing ─────────────────────────────────────────────────────

pub fn strip_md_images(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '!' && chars.peek() == Some(&'[') {
            chars.next();
            let mut alt = String::new();
            let mut depth = 1;
            for ch in chars.by_ref() {
                if ch == '[' {
                    depth += 1;
                } else if ch == ']' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                alt.push(ch);
            }
            if chars.peek() == Some(&'(') {
                chars.next();
                let mut pdepth = 1;
                for ch in chars.by_ref() {
                    if ch == '(' {
                        pdepth += 1;
                    } else if ch == ')' {
                        pdepth -= 1;
                        if pdepth == 0 {
                            break;
                        }
                    }
                }
            }
            if !alt.is_empty() {
                use std::fmt::Write;
                let _ = write!(out, "[figure: {alt}]");
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn strip_paired(s: &str, delim: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(delim) {
        out.push_str(&rest[..start]);
        rest = &rest[start + delim.len()..];
        match rest.find(delim) {
            Some(end) => {
                out.push_str(&rest[..end]);
                rest = &rest[end + delim.len()..];
            }
            None => out.push_str(delim),
        }
    }
    out.push_str(rest);
    out
}

pub fn strip_md_formatting(s: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            lines.push(line.to_string());
            continue;
        }
        if trimmed == "$$" {
            continue;
        }
        let line = if trimmed.starts_with('#') {
            trimmed.trim_start_matches('#').trim_start().to_string()
        } else if trimmed.starts_with('>') {
            trimmed.trim_start_matches('>').trim_start().to_string()
        } else {
            line.to_string()
        };
        let line = strip_paired(&line, "**");
        let line = strip_paired(&line, "*");
        let line = strip_paired(&line, "$");
        let line = strip_paired(&line, "`");
        lines.push(line);
    }
    lines.join("\n")
}

pub fn strip_md_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('\n') | None => {}
                Some(&next) if next.is_ascii_punctuation() => {
                    out.push(next);
                    chars.next();
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── html processing ─────────────────────────────────────────────────────────

pub fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        let Some(gt) = rest.find('>') else {
            out.push_str(rest);
            return out;
        };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        // markdown auto-link: <https://...> or <http://...>
        if tag.starts_with("https://") || tag.starts_with("http://") {
            out.push_str(tag);
            continue;
        }
        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.starts_with("a ") || tag_lower.starts_with("a\t") {
            if let Some(href) = extract_href(tag) {
                if let Some(close) = rest.to_ascii_lowercase().find("</a>") {
                    let text = rest[..close].trim();
                    rest = &rest[close + 4..];
                    if text.is_empty() {
                        out.push_str(&href);
                    } else {
                        out.push_str(text);
                        out.push_str(" (");
                        out.push_str(&href);
                        out.push(')');
                    }
                    continue;
                }
            }
        }
        if tag_lower == "sup" {
            if let Some(close) = rest.to_ascii_lowercase().find("</sup>") {
                let inner = &rest[..close];
                rest = &rest[close + 6..];
                let mapped: Option<String> = inner.chars().map(sup_char).collect();
                out.push_str(&mapped.unwrap_or_else(|| format!("^{inner}")));
                continue;
            }
        }
        if tag_lower == "sub" {
            if let Some(close) = rest.to_ascii_lowercase().find("</sub>") {
                let inner = &rest[..close];
                rest = &rest[close + 6..];
                let mapped: Option<String> = inner.chars().map(sub_char).collect();
                out.push_str(&mapped.unwrap_or_else(|| format!("_{inner}")));
                continue;
            }
        }
        // <br> / <br/> → newline
        if tag_lower == "br" || tag_lower == "br/" || tag_lower == "br /" {
            out.push('\n');
            continue;
        }
        // <p> → double newline
        if (tag_lower == "p" || tag_lower.starts_with("p "))
            && !out.is_empty()
            && !out.ends_with('\n')
        {
            out.push_str("\n\n");
        }
        // all other tags: drop
    }
    out.push_str(rest);
    out
}

fn extract_href(tag: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pos = lower.find("href=")?;
    let after = &tag[pos + 5..];
    let (delim, start) = match after.as_bytes().first()? {
        b'"' => ('"', 1),
        b'\'' => ('\'', 1),
        _ => return None,
    };
    let end = after[start..].find(delim)?;
    let url = after[start..start + end].trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}

const fn sup_char(c: char) -> Option<char> {
    match c {
        '0' => Some('\u{2070}'),
        '1' => Some('\u{00B9}'),
        '2' => Some('\u{00B2}'),
        '3' => Some('\u{00B3}'),
        '4' => Some('\u{2074}'),
        '5' => Some('\u{2075}'),
        '6' => Some('\u{2076}'),
        '7' => Some('\u{2077}'),
        '8' => Some('\u{2078}'),
        '9' => Some('\u{2079}'),
        '+' => Some('\u{207A}'),
        '-' => Some('\u{207B}'),
        '=' => Some('\u{207C}'),
        '(' => Some('\u{207D}'),
        ')' => Some('\u{207E}'),
        'n' => Some('\u{207F}'),
        'i' => Some('\u{2071}'),
        'T' => Some('\u{1D40}'),
        ' ' => Some(' '),
        _ => None,
    }
}

const fn sub_char(c: char) -> Option<char> {
    match c {
        '0' => Some('\u{2080}'),
        '1' => Some('\u{2081}'),
        '2' => Some('\u{2082}'),
        '3' => Some('\u{2083}'),
        '4' => Some('\u{2084}'),
        '5' => Some('\u{2085}'),
        '6' => Some('\u{2086}'),
        '7' => Some('\u{2087}'),
        '8' => Some('\u{2088}'),
        '9' => Some('\u{2089}'),
        '+' => Some('\u{208A}'),
        '-' => Some('\u{208B}'),
        '=' => Some('\u{208C}'),
        '(' => Some('\u{208D}'),
        ')' => Some('\u{208E}'),
        'a' => Some('\u{2090}'),
        'e' => Some('\u{2091}'),
        'h' => Some('\u{2095}'),
        'i' => Some('\u{1D62}'),
        'j' => Some('\u{2C7C}'),
        'k' => Some('\u{2096}'),
        'l' => Some('\u{2097}'),
        'm' => Some('\u{2098}'),
        'n' => Some('\u{2099}'),
        'o' => Some('\u{2092}'),
        'p' => Some('\u{209A}'),
        'r' => Some('\u{1D63}'),
        's' => Some('\u{209B}'),
        't' => Some('\u{209C}'),
        'u' => Some('\u{1D64}'),
        'v' => Some('\u{1D65}'),
        'x' => Some('\u{2093}'),
        ' ' => Some(' '),
        _ => None,
    }
}

// ── bibtex sanitization ─────────────────────────────────────────────────────

pub fn sanitize_bibtex(s: &str) -> String {
    let mut out = s.to_string();
    // fix year={Thu Feb 19 2026 11:18:12 GMT+0000 (...)} → year={2026}
    if let Some(start) = out.find("year={") {
        if let Some(end) = out[start..].find('}') {
            let val = &out[start + 6..start + end];
            for word in val.split_whitespace() {
                if word.len() == 4 {
                    if let Ok(y) = word.parse::<u32>() {
                        if (1900..=2100).contains(&y) {
                            out = format!(
                                "{}year={{{}}}{}",
                                &out[..start],
                                y,
                                &out[start + end + 1..]
                            );
                            break;
                        }
                    }
                }
            }
        }
    }
    // fix key: @misc{heekThu Feb 19 2026 ... (Coordinated Universal Time)rest,
    if let Some(brace) = out.find('{') {
        if let Some(comma) = out[brace..].find(',') {
            let key = &out[brace + 1..brace + comma];
            if key.contains("GMT") {
                for day in ["Mon ", "Tue ", "Wed ", "Thu ", "Fri ", "Sat ", "Sun "] {
                    if let Some(dp) = key.find(day) {
                        if let Some(paren) = key[dp..].find(')') {
                            let clean = format!("{}{}", &key[..dp], &key[dp + paren + 1..]);
                            out = format!(
                                "{}{{{},{}",
                                &out[..brace],
                                clean,
                                &out[brace + comma + 1..]
                            );
                            break;
                        }
                    }
                }
            }
        }
    }
    out
}

// ── text utilities ──────────────────────────────────────────────────────────


pub fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── clean text pipelines ────────────────────────────────────────────────────

pub fn clean_comment(text: &str) -> String {
    let s = strip_html_tags(text);
    let s = strip_md_escapes(&s);
    decode_html_entities(&s)
}

pub fn clean_overview(text: &str) -> String {
    let s = strip_md_images(text);
    let s = strip_md_formatting(&s);
    decode_html_entities(&s)
}
