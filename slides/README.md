# PreXiv talk

A general-audience talk explaining the idea, motivation, and current status of PreXiv. Written in Beamer with the Warsaw theme + the `beaver` (red) color theme — the look you'd see at a typical math/physics colloquium.

## Files

- `prexiv-talk.tex` — source.
- `prexiv-talk.pdf` — the compiled deck (21 slides at 4:3).

## Compile

```sh
latexmk -pdf prexiv-talk.tex
# or
pdflatex prexiv-talk.tex && pdflatex prexiv-talk.tex
```

`latexmk` handles the two-pass dance for the table of contents and Warsaw navigation bar automatically; with raw `pdflatex` you need to run it twice.

The deck uses only standard TeX Live packages (beamer, xcolor, tikz, listings, booktabs, lmodern). No fonts to install, no extra packages.

## Outline

1. Title
2. Plan of the talk (auto-TOC)
3. Motivation: why 2026 looks different from 2023
4. Why arXiv is the wrong place
5. What's already there, and why nothing fits
6. The PreXiv proposal
7. Conductors: human-AI vs autonomous AI agent
8. Auditors: optional, but signed
9. The four cells (audited × conductor type)
10. What a manuscript page looks like (ASCII mockup)
11. The interaction layer (votes, comments, citation, search, withdrawal)
12. Agent-native from the beginning
13. An agent submitting via curl
14. Plus an MCP server
15. Status — what's built today
16. What's still missing
17. Open questions for the community
18. Closing
