//! Render user-supplied markdown to safe HTML.
//!
//! Comments, abstracts, conductor notes, and auditor statements all flow
//! through this. Output is HTML-sanitised against a fixed allowlist so a
//! commenter can't sneak `<script>` or `onerror=` into the page. KaTeX
//! ($…$, $$…$$) is left alone — pulldown-cmark passes `$` through as text,
//! and the auto-render init in `/static/js/katex-init.js` typesets it on
//! the client after the HTML lands in the DOM.

use ammonia::Builder;
use pulldown_cmark::{html, Options, Parser};

/// Render `input` from GitHub-flavoured markdown (tables, strikethrough,
/// tasklists, fenced code, autolinks) into sanitised HTML.
pub fn render(input: &str) -> String {
    // GFM-ish: single newlines become hard breaks (matches the JS app's
    // marked({ breaks: true }) behaviour) by post-processing soft breaks.
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(input, opts).map(|event| {
        use pulldown_cmark::Event;
        match event {
            // Treat single newlines as hard breaks (GFM `breaks: true`).
            Event::SoftBreak => Event::HardBreak,
            e => e,
        }
    });
    let mut raw_html = String::new();
    html::push_html(&mut raw_html, parser);

    Builder::default()
        .add_tag_attributes("a", &["href", "title", "target"])
        .link_rel(Some("nofollow ugc noopener"))
        .add_generic_attributes(&["class"])
        // Trust class names so we can keep tasklist styling etc.
        .clean(&raw_html)
        .to_string()
}
