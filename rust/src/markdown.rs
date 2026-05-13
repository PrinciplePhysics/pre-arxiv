//! Render user-supplied markdown to safe HTML.
//!
//! Comments, abstracts, conductor notes, and auditor statements all flow
//! through this. Output is HTML-sanitised against a fixed allowlist so a
//! commenter can't sneak `<script>` or `onerror=` into the page.
//!
//! ## KaTeX-aware
//!
//! `$…$`, `\(...\)` (inline) and `$$…$$`, `\[...\]` (display) math are
//! common in PreXiv content.
//! Plain markdown rendering ruins them: CommonMark's `_emphasis_` rule
//! happily opens an italic at the underscore inside `\mathrm{Var}_\Psi`,
//! and then keeps it open until it finds a closing `_` elsewhere on the
//! page — which is usually another underscore inside the *next* `$…$`
//! block. The text in between is eaten as italic and the `$` boundaries
//! become stray dollar signs that KaTeX can't pair up.
//!
//! Fix: before markdown runs, scan the input and lift every math region
//! out into a side buffer, leaving an ASCII placeholder
//! token (`\u{FDD0}MATH<n>\u{FDD1}`, a private-use codepoint pair that
//! cannot occur in normal text and that pulldown-cmark + ammonia both
//! pass through verbatim). Run markdown + ammonia, then splice the math
//! back. The result reaches the browser with intact `$…$` markers; the
//! KaTeX auto-render init in /static/js/katex-init.js handles them.

use ammonia::Builder;
use pulldown_cmark::{html, Options, Parser};

/// Inline-only render for places like manuscript titles, where wrapping
/// the whole output in <p>…</p> would produce invalid HTML inside <h1>.
/// Strips a single outer paragraph wrapper if present.
pub fn render_inline(input: &str) -> String {
    let mut out = render(input);
    let trimmed = out.trim_end_matches('\n');
    if trimmed.starts_with("<p>") && trimmed.ends_with("</p>") {
        let inner = &trimmed[3..trimmed.len() - 4];
        if !inner.contains("<p>") {
            out = inner.to_string();
        }
    }
    out
}

/// Render `input` from GitHub-flavoured markdown (tables, strikethrough,
/// tasklists, fenced code, autolinks) into sanitised HTML.
pub fn render(input: &str) -> String {
    let (substituted, math) = extract_math(input);
    let substituted = render_latex_text_commands(&substituted);

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(&substituted, opts).map(|event| {
        use pulldown_cmark::Event;
        match event {
            Event::SoftBreak => Event::HardBreak,
            e => e,
        }
    });
    let mut raw_html = String::new();
    html::push_html(&mut raw_html, parser);

    let sanitized = Builder::default()
        .add_tag_attributes("a", &["href", "title", "target"])
        .link_rel(Some("nofollow ugc noopener"))
        .add_generic_attributes(&["class"])
        .clean(&raw_html)
        .to_string();

    restore_math(sanitized, &math)
}

/// Convert a deliberately tiny subset of LaTeX text-mode commands that
/// commonly appear when authors paste an abstract from a `.tex` source.
/// Math regions are extracted before this runs, so commands inside
/// `$...$`, `\(...\)`, etc. remain untouched for KaTeX.
fn render_latex_text_commands(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] == '\\' {
            let cmd_start = i + 1;
            let mut cmd_end = cmd_start;
            while cmd_end < chars.len() && chars[cmd_end].is_ascii_alphabetic() {
                cmd_end += 1;
            }
            if cmd_end > cmd_start && cmd_end < chars.len() && chars[cmd_end] == '{' {
                let cmd: String = chars[cmd_start..cmd_end].iter().collect();
                if let Some((open, close)) = latex_text_tag(&cmd) {
                    if let Some((inner, next_i)) = take_braced(&chars, cmd_end) {
                        out.push_str(open);
                        out.push_str(&render_latex_text_commands(&inner));
                        out.push_str(close);
                        i = next_i;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

fn latex_text_tag(cmd: &str) -> Option<(&'static str, &'static str)> {
    match cmd {
        "emph" | "textit" => Some(("<em>", "</em>")),
        "textbf" => Some(("<strong>", "</strong>")),
        "texttt" => Some(("<code>", "</code>")),
        _ => None,
    }
}

fn take_braced(chars: &[char], open_i: usize) -> Option<(String, usize)> {
    if chars.get(open_i) != Some(&'{') {
        return None;
    }
    let mut out = String::new();
    let mut depth = 0i32;
    let mut i = open_i;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            out.push(c);
            if let Some(next) = chars.get(i + 1) {
                out.push(*next);
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if c == '{' {
            depth += 1;
            if depth > 1 {
                out.push(c);
            }
            i += 1;
            continue;
        }
        if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((out, i + 1));
            }
            if depth < 0 {
                return None;
            }
            out.push(c);
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    None
}

const PLACE_OPEN: char = '\u{FDD0}';
const PLACE_CLOSE: char = '\u{FDD1}';

/// Walk the input, lifting TeX math-delimited regions out. Returns the
/// substituted text (with placeholders) and the ordered list of math
/// fragments. Each fragment retains its original delimiters so the
/// browser-side KaTeX auto-render finds them as written.
fn extract_math(input: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut math: Vec<String> = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(open) = chars.peek().copied().filter(|c| matches!(c, '(' | '[')) {
                chars.next();
                let close = if open == '(' { ')' } else { ']' };
                let mut content = String::new();
                let mut closed = false;
                while let Some(d) = chars.next() {
                    if d == '\\' {
                        if chars.peek() == Some(&close) {
                            chars.next();
                            closed = true;
                            break;
                        }
                        content.push(d);
                        if let Some(next) = chars.next() {
                            content.push(next);
                        }
                        continue;
                    }
                    content.push(d);
                }
                if closed {
                    push_placeholder(&mut out, math.len());
                    math.push(format!("\\{open}{content}\\{close}"));
                } else {
                    out.push('\\');
                    out.push(open);
                    out.push_str(&content);
                }
                continue;
            }
            // Verbatim copy the next char so `\$` doesn't accidentally
            // open a math region. (Authors who *want* a literal $ in
            // prose write \$.)
            out.push(c);
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if c != '$' {
            out.push(c);
            continue;
        }
        // c == '$'. Block ($$) or inline ($)?
        if chars.peek() == Some(&'$') {
            // Display math.
            chars.next(); // consume second $
            let mut content = String::new();
            let mut closed = false;
            while let Some(d) = chars.next() {
                if d == '\\' {
                    content.push(d);
                    if let Some(next) = chars.next() {
                        content.push(next);
                    }
                    continue;
                }
                if d == '$' && chars.peek() == Some(&'$') {
                    chars.next();
                    closed = true;
                    break;
                }
                content.push(d);
            }
            if closed {
                push_placeholder(&mut out, math.len());
                math.push(format!("$${content}$$"));
            } else {
                out.push('$');
                out.push('$');
                out.push_str(&content);
            }
        } else {
            // Inline math. Don't cross a blank line (two newlines).
            let mut content = String::new();
            let mut closed = false;
            let mut prev_was_newline = false;
            while let Some(d) = chars.next() {
                if d == '\\' {
                    content.push(d);
                    if let Some(next) = chars.next() {
                        content.push(next);
                    }
                    prev_was_newline = false;
                    continue;
                }
                if d == '$' {
                    closed = true;
                    break;
                }
                if d == '\n' {
                    if prev_was_newline {
                        // Blank line — abort the inline-math run.
                        content.push('\n');
                        break;
                    }
                    prev_was_newline = true;
                } else {
                    prev_was_newline = false;
                }
                content.push(d);
            }
            if closed {
                push_placeholder(&mut out, math.len());
                math.push(format!("${content}$"));
            } else {
                out.push('$');
                out.push_str(&content);
            }
        }
    }
    (out, math)
}

fn push_placeholder(out: &mut String, n: usize) {
    out.push(PLACE_OPEN);
    out.push_str("MATH");
    out.push_str(&n.to_string());
    out.push(PLACE_CLOSE);
}

fn restore_math(mut html: String, math: &[String]) -> String {
    for (i, m) in math.iter().enumerate() {
        let placeholder = format!("{PLACE_OPEN}MATH{i}{PLACE_CLOSE}");
        html = html.replace(&placeholder, m);
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_math_with_underscores_survives() {
        let out = render(r"variance $\mathrm{Var}_\Psi(N_g)$, and $\mathrm{Var}_\Psi(J_g)$.");
        // The exact $-pair must still be in the output, in the right order.
        assert!(out.contains(r"$\mathrm{Var}_\Psi(N_g)$"));
        assert!(out.contains(r"$\mathrm{Var}_\Psi(J_g)$"));
        assert!(!out.contains("<em>"));
    }

    #[test]
    fn display_math_block_survives() {
        let src = r"see $$E_{\mathrm{xc}}(\Psi) \ge -\int F(\rho)\,dx$$ end";
        let out = render(src);
        assert!(out.contains(r"$$E_{\mathrm{xc}}(\Psi) \ge -\int F(\rho)\,dx$$"));
    }

    #[test]
    fn latex_paren_math_survives_markdown_escaping() {
        let src = r"Let \(\rho_\Psi(x)\) and \(T_\Psi(x)\) denote densities.";
        let out = render(src);
        assert!(out.contains(r"\(\rho_\Psi(x)\)"));
        assert!(out.contains(r"\(T_\Psi(x)\)"));
        assert!(!out.contains(r"(\rho_\Psi(x))"));
    }

    #[test]
    fn latex_bracket_math_survives_markdown_escaping() {
        let src = r"Display \[ C_{\mathrm{cell}}:=\tfrac{3}{5} \] done";
        let out = render(src);
        assert!(out.contains(r"\[ C_{\mathrm{cell}}:=\tfrac{3}{5} \]"));
    }

    #[test]
    fn escaped_dollar_does_not_open_math() {
        let out = render(r"price: \$5 vs \$10");
        // The math placeholders shouldn't appear.
        assert!(!out.contains('\u{FDD0}'));
    }

    #[test]
    fn latex_text_emph_renders_as_html_emphasis() {
        let out = render(r"controlled \emph{without} any spectral gap assumption");
        assert!(out.contains("<em>without</em>"));
        assert!(!out.contains(r"\emph{without}"));
    }

    #[test]
    fn latex_text_commands_leave_math_regions_alone() {
        let out = render(r"$\emph{x}$ and \textbf{bold text} plus \texttt{code}");
        assert!(out.contains(r"$\emph{x}$"));
        assert!(out.contains("<strong>bold text</strong>"));
        assert!(out.contains("<code>code</code>"));
    }

    #[test]
    fn nested_latex_text_commands_render() {
        let out = render(r"\emph{outer \textbf{inner}}");
        assert!(out.contains("<em>outer <strong>inner</strong></em>"));
    }

    #[test]
    fn unclosed_dollar_falls_through() {
        let out = render(r"unclosed: $foo bar");
        // Should not crash; literal $ stays.
        assert!(out.contains("$foo bar"));
    }

    #[test]
    fn unrelated_markdown_still_renders() {
        let out = render("**bold** and `code` and $x = 1$ together.");
        assert!(out.contains("<strong>bold</strong>"));
        assert!(out.contains("<code>code</code>"));
        assert!(out.contains("$x = 1$"));
    }
}
