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
```

**Look up a paper.** Give it an ID, a URL, a DOI — it figures it out:
```bash
alphaxiv paper 2502.11089
alphaxiv paper "https://arxiv.org/abs/1706.03762"
alphaxiv paper "https://doi.org/10.48550/arXiv.1706.03762"
```

This hits five APIs at once (alphaxiv, Semantic Scholar, HuggingFace, OpenAlex, and arxiv itself) and gives you everything back in one shot. Abstract, authors, bibtex, citation counts, TLDR, DOI, journal info, open access status, retraction warnings, associated HuggingFace models and datasets, comments, github repos. Add `--overview` if you also want the alphaxiv blog post about the paper.

**Read the full text:**
```bash
alphaxiv read 1706.03762
```

Pulls the HTML version from arxiv, parses the LaTeXML, and gives you the whole paper split into sections with equations as LaTeX. No PDF parsing.

**Search:**
```bash
alphaxiv search "chain of thought reasoning"
alphaxiv search "diffusion models" --limit 5
```

Results come back with the full abstract, not a snippet.

**Citations and references:**
```bash
alphaxiv refs 1706.03762      # what this paper cites
alphaxiv cites 1706.03762     # what cites this paper
```

**Batch lookups:**
```bash
alphaxiv batch 1706.03762 2502.11089 2501.12948 --no-comments
```

**Bibtex:**
```bash
alphaxiv paper 1706.03762 --bibtex
```

## JSON

Add `--json` to any command. The schema is stable — every field shows up every time, even when empty. Errors come back as `{"error": "..."}` with exit code 1. If you're feeding this into an agent or a script, that's what you want.

```bash
alphaxiv paper 2502.11089 --json
alphaxiv feed --limit 5 --json
```

`--raw` keeps markdown/html intact instead of stripping it for the terminal.

## Data sources

When you look up a paper, it queries all of these in parallel:

- **alphaxiv.org** — feed, search, comments, overviews, view counts, bibtex, github
- **Semantic Scholar** — TLDR, citations, references, DOI, journal, publication type, fields of study
- **HuggingFace** — paper upvotes, models, datasets, spaces
- **OpenAlex** — retraction status, open access classification, topic hierarchy
- **arxiv.org** — full paper HTML

## Install

```
cargo install --path .
```

Needs Rust 1.85+.
