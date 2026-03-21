# alphaxiv

Read arxiv papers from your terminal.

You can look up papers, read the full text, search, browse what's trending, see who cites what — and get it all as json for piping into other tools or LLM agents.

```
cargo install --path .
```

## What you can do

**See what's trending:**
```bash
alphaxiv feed
alphaxiv feed --sort views --interval 30d --limit 10
```

**Look up a paper** — give it an id, a url, a doi, whatever:
```bash
alphaxiv paper 2502.11089
alphaxiv paper "https://arxiv.org/abs/1706.03762"
alphaxiv paper "https://doi.org/10.48550/arXiv.1706.03762"
alphaxiv paper "arXiv:2502.11089"
```

You get the abstract, authors, comments, bibtex, github links, plus citation count and a one-line TLDR from Semantic Scholar. Add `--overview` to also pull the alphaxiv blog post about the paper.

**Read the full paper:**
```bash
alphaxiv read 1706.03762
```

This grabs the HTML version from arxiv and gives you the whole thing — introduction, methods, experiments, all of it — split into sections. Equations come through as LaTeX. No janky PDF parsing.

**Search for papers:**
```bash
alphaxiv search "chain of thought reasoning"
alphaxiv search "diffusion models" --limit 5
```

Every result comes back with the full abstract, not a useless snippet.

**See what a paper cites, or who cites it:**
```bash
alphaxiv refs 1706.03762      # papers this one references
alphaxiv cites 1706.03762     # papers that cite this one
```

**Look up a bunch of papers at once:**
```bash
alphaxiv batch 1706.03762 2502.11089 2501.12948 --no-comments
```

Runs in parallel, so it's fast.

## JSON mode

Add `--json` to any command. You get clean json on stdout with a stable schema — every field shows up every time, no fields disappearing when they're empty. Errors come back as `{"error": "..."}` with exit code 1.

```bash
alphaxiv paper 2502.11089 --json
alphaxiv read 1706.03762 --json
alphaxiv cites 2502.11089 --json
```

If you're building something on top of this (an agent, a script, whatever), `--json` is what you want.

There's also `--raw` which keeps markdown and html formatting intact instead of stripping it for the terminal.

## Where the data comes from

- **alphaxiv.org** — trending feed, search, comments, overviews, view/like counts
- **arxiv.org** — full paper text (their HTML rendering)
- **Semantic Scholar** — TLDR, citation counts, references, who-cites-who

## Install

```
cargo install --path .
```

That puts `alphaxiv` in your path. Needs Rust.
