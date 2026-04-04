# alphaxiv

A CLI for looking up arxiv papers. You give it a paper ID and it tells you everything it can find — who wrote it, how many citations, whether anyone's put models on HuggingFace for it, if it's been retracted, all of it. Works as a pipe-friendly JSON source for agents and scripts too.

```
cargo install --path .
```

## What you can do

**See what's trending:**
```bash
alphaxiv feed
alphaxiv feed --sort views --interval 30d --limit 10
alphaxiv feed --sort twitter --limit 10
```

**Browse latest papers in a category:**
```bash
alphaxiv new cs.AI
alphaxiv new hep-th --limit 10
alphaxiv new cs.CL --from 2024-06-01 --to 2024-06-30
```

Hits the arxiv API directly, sorted by submission date. Supports date ranges.

**Look up a paper.** Give it an ID, a URL, a DOI — it figures it out:
```bash
alphaxiv paper 2502.11089
alphaxiv paper "https://arxiv.org/abs/1706.03762"
alphaxiv paper "https://doi.org/10.48550/arXiv.1706.03762"
```

This hits five APIs at once (alphaxiv, Semantic Scholar, HuggingFace, OpenAlex, and arxiv itself) and gives you everything back in one shot. Abstract, authors, bibtex, citation counts, TLDR, DOI, journal info, open access status, retraction warnings, associated HuggingFace models and datasets, comments, github repos. Add `--overview` if you also want the alphaxiv blog post about the paper.

If any enrichment API is down or rate-limited, you'll see a warning on stderr instead of silently missing data:
```
warning: semantic scholar: failed after 3 retries: http 429
```

**Read the full text:**
```bash
alphaxiv read 1706.03762
```

Pulls the HTML version from arxiv, parses the LaTeXML, and gives you the whole paper split into sections with equations as LaTeX. No PDF parsing.

**Download the PDF:**
```bash
alphaxiv download 1706.03762
alphaxiv download 1706.03762 -o attention.pdf
```

**Search:**
```bash
alphaxiv search "chain of thought reasoning"
alphaxiv search "diffusion models" --limit 5
alphaxiv search "transformers" --sort submitted              # newest first
alphaxiv search "attention" --sort submitted --from 2024-01-01 --to 2024-06-30
```

Default search uses alphaxiv for relevance ranking. `--sort submitted` or `--sort updated` switches to the arxiv API with date sorting and optional date range filtering.

**Look up an author:**
```bash
alphaxiv author "Yann LeCun"
alphaxiv author "Geoffrey Hinton" --limit 10
```

Shows h-index, citation count, paper count, and their top papers via Semantic Scholar.

**Find similar papers:**
```bash
alphaxiv similar 2502.11089
```

Uses Semantic Scholar's recommendations API.

**Find related papers:**
```bash
alphaxiv related 1706.03762
```

Uses OpenAlex's related works graph.

**Citations and references:**
```bash
alphaxiv refs 1706.03762      # what this paper cites
alphaxiv cites 1706.03762     # what cites this paper
```

Citation results include context sentences showing exactly how the paper was cited.

**Batch lookups:**
```bash
alphaxiv batch 1706.03762 2502.11089 2501.12948 --no-comments
```

**Bibtex:**
```bash
alphaxiv paper 1706.03762 --bibtex
```

## Output modes

**JSON** — add `--json` to any command. The schema is stable — every field shows up every time, even when empty. Errors come back as `{"error": "..."}` with exit code 1. Paper lookups include a `warnings` array for any enrichment API failures.

```bash
alphaxiv paper 2502.11089 --json
alphaxiv feed --limit 5 --json
```

**IDs only** — add `--ids` for pipe-friendly output. Just paper IDs, one per line.

```bash
alphaxiv feed --ids | head -5 | xargs alphaxiv batch
alphaxiv new cs.AI --ids
alphaxiv cites 1706.03762 --ids
```

**Raw** — `--raw` keeps markdown/html intact instead of stripping it for the terminal.

## Data sources

When you look up a paper, it queries all of these in parallel:

- **alphaxiv.org** — feed, search, comments, overviews, view counts, bibtex, github
- **Semantic Scholar** — TLDR, citations, references, similar papers, author profiles, DOI, journal, fields of study
- **HuggingFace** — paper upvotes, models, datasets, spaces
- **OpenAlex** — retraction status, open access classification, topic hierarchy, related works
- **arxiv.org** — full paper HTML, category browsing, date-sorted search, PDF download

## Install

```
cargo install --path .
```

Needs Rust 1.85+.
