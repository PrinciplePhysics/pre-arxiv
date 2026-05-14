//! Server-side LaTeX → PDF compilation, arXiv style.
//!
//! Input: a byte buffer that is either a single `.tex` file or an archive
//! (`.zip` or `.tar.gz` / `.tgz`).
//! Output: `Ok((pdf_bytes, log_excerpt))` on success, `Err(CompileError)`
//! with the user-facing log excerpt on failure.
//!
//! Safety:
//!   * pdflatex runs with `--no-shell-escape --interaction=nonstopmode`
//!   * total wall-clock cap of 60 seconds per attempt (90 s for the
//!     three-pass run that covers cross-refs / bibtex)
//!   * compile happens in a fresh per-submission temp dir, so concurrent
//!     submissions can't see each other's files

use std::ffi::OsStr;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const PDFLATEX_TIMEOUT: Duration = Duration::from_secs(60);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_LOG_BYTES: usize = 32 * 1024;
const BLACKOUT_TEX: &str = r"\rule{6em}{1.1ex}";

#[derive(Debug)]
pub struct Compiled {
    pub pdf: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct RedactionOptions {
    pub hide_human: bool,
    pub hide_ai_model: bool,
    pub human_name: Option<String>,
    pub ai_models: Vec<String>,
}

impl RedactionOptions {
    pub fn any(&self) -> bool {
        self.hide_human || self.hide_ai_model
    }
}

#[derive(Debug)]
pub struct PreparedSource {
    pub filename: String,
    pub data: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("uploaded file is empty")]
    Empty,
    #[error("no \\documentclass found in any of the uploaded .tex files")]
    NoMainTex,
    #[error("the archive contains no .tex files")]
    NoTexFiles,
    #[error("archive extraction failed: {0}")]
    Extract(String),
    #[error("compilation timed out (>{} seconds)", TOTAL_TIMEOUT.as_secs())]
    Timeout,
    #[error("LaTeX failed; no PDF produced")]
    NoPdf { log: String },
    #[error("internal error during compile: {0}")]
    Other(String),
}

impl CompileError {
    /// User-facing log excerpt (last MAX_LOG_BYTES of the compile log,
    /// for the "Show LaTeX log" disclosure on the error page).
    pub fn log(&self) -> Option<&str> {
        match self {
            CompileError::NoPdf { log } => Some(log.as_str()),
            _ => None,
        }
    }
}

/// Prepare a source upload for public storage and compilation. If privacy
/// options are set, the returned source has identity fields blacked out and
/// is the only source artifact callers should persist.
pub fn prepare_source(
    filename: &str,
    data: &[u8],
    redaction: &RedactionOptions,
) -> Result<PreparedSource, CompileError> {
    if data.is_empty() {
        return Err(CompileError::Empty);
    }
    if !redaction.any() {
        return Ok(PreparedSource {
            filename: filename.to_string(),
            data: data.to_vec(),
        });
    }

    let kind = detect_kind(filename, data);
    match kind {
        SourceKind::SingleTex => Ok(PreparedSource {
            filename: redacted_source_filename(filename, SourceKind::SingleTex),
            data: redact_text_bytes(data, redaction),
        }),
        SourceKind::Zip => Ok(PreparedSource {
            filename: redacted_source_filename(filename, SourceKind::Zip),
            data: redact_zip(data, redaction).map_err(|e| CompileError::Extract(format!("{e}")))?,
        }),
        SourceKind::TarGz => Ok(PreparedSource {
            filename: redacted_source_filename(filename, SourceKind::TarGz),
            data: redact_targz(data, redaction)
                .map_err(|e| CompileError::Extract(format!("{e}")))?,
        }),
        SourceKind::Unknown => Err(CompileError::Other(format!(
            "unrecognised source format (expected .tex / .zip / .tar.gz); got filename '{filename}'"
        ))),
    }
}

/// Detect what kind of upload `data` is, by looking at its filename
/// extension (and as a tie-breaker, the magic bytes for zip / gzip).
pub fn detect_kind(filename: &str, data: &[u8]) -> SourceKind {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".tex") {
        SourceKind::SingleTex
    } else if lower.ends_with(".zip") || data.starts_with(b"PK\x03\x04") {
        SourceKind::Zip
    } else if lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || data.starts_with(&[0x1f, 0x8b])
    {
        SourceKind::TarGz
    } else {
        SourceKind::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    SingleTex,
    Zip,
    TarGz,
    Unknown,
}

// ── source redaction ────────────────────────────────────────────────

fn redact_zip(data: &[u8], redaction: &RedactionOptions) -> Result<Vec<u8>> {
    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).context("opening zip")?;
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("reading zip entry")?;
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => continue,
        };
        if rel.as_os_str().is_empty() {
            continue;
        }

        let zip_name = path_to_archive_name(&rel)?;
        let mut options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if let Some(mode) = entry.unix_mode() {
            options = options.unix_permissions(mode);
        }

        if entry.is_dir() {
            writer
                .add_directory(zip_name, options)
                .context("write zip dir")?;
            continue;
        }

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).context("read zip entry")?;
        if should_redact_text_file(&rel) {
            buf = redact_text_bytes(&buf, redaction);
        }
        writer
            .start_file(zip_name, options)
            .context("write zip file")?;
        writer.write_all(&buf).context("write zip bytes")?;
    }

    let cursor = writer.finish().context("finish zip")?;
    Ok(cursor.into_inner())
}

fn redact_targz(data: &[u8], redaction: &RedactionOptions) -> Result<Vec<u8>> {
    let gz = flate2::read::GzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(gz);
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);

    for entry in archive.entries().context("opening tar")? {
        let mut entry = entry.context("reading tar entry")?;
        let rel = entry.path().context("entry path")?.into_owned();
        if !is_safe_relative_path(&rel) || !entry.header().entry_type().is_file() {
            continue;
        }

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).context("read tar entry")?;
        if should_redact_text_file(&rel) {
            buf = redact_text_bytes(&buf, redaction);
        }

        let mut header = entry.header().clone();
        header.set_path(&rel).context("set tar path")?;
        header.set_size(buf.len() as u64);
        header.set_cksum();
        builder
            .append(&header, Cursor::new(buf))
            .context("append tar entry")?;
    }

    let encoder = builder.into_inner().context("finish tar")?;
    encoder.finish().context("finish gzip")
}

fn redact_text_bytes(data: &[u8], redaction: &RedactionOptions) -> Vec<u8> {
    match std::str::from_utf8(data) {
        Ok(s) => redact_text(s, redaction).into_bytes(),
        Err(_) => data.to_vec(),
    }
}

fn redact_text(input: &str, redaction: &RedactionOptions) -> String {
    let mut out = input.to_string();

    if redaction.hide_human {
        for command in HUMAN_IDENTITY_COMMANDS {
            out = redact_latex_command_arguments(&out, command);
        }
    }

    for term in redaction_terms(redaction) {
        out = out.replace(&term, BLACKOUT_TEX);
    }

    out
}

const HUMAN_IDENTITY_COMMANDS: &[&str] = &[
    "author",
    "authors",
    "address",
    "affiliation",
    "affiliations",
    "altaffiliation",
    "email",
    "ead",
    "correspondingauthor",
    "institute",
    "institution",
    "streetaddress",
    "city",
    "state",
    "country",
    "postcode",
    "zipcode",
    "authorinfo",
    "authorrunning",
    "shortauthors",
    "IEEEauthorblockN",
    "IEEEauthorblockA",
    "orcid",
    "homepage",
    "thanks",
];

fn redaction_terms(redaction: &RedactionOptions) -> Vec<String> {
    let mut terms = Vec::new();
    if redaction.hide_human {
        if let Some(name) = redaction.human_name.as_deref() {
            push_term(&mut terms, name);
        }
    }
    if redaction.hide_ai_model {
        for model in &redaction.ai_models {
            push_term(&mut terms, model);
        }
    }
    terms.sort_by_key(|s| std::cmp::Reverse(s.len()));
    terms.dedup();
    terms
}

fn push_term(terms: &mut Vec<String>, term: &str) {
    let trimmed = term.trim();
    if trimmed.chars().count() >= 2 {
        terms.push(trimmed.to_string());
    }
}

fn redact_latex_command_arguments(input: &str, command: &str) -> String {
    let needle = format!("\\{command}");
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;

    while let Some(found) = input[cursor..].find(&needle) {
        let start = cursor + found;
        let command_end = start + needle.len();
        if bytes
            .get(command_end)
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'@')
        {
            out.push_str(&input[cursor..command_end]);
            cursor = command_end;
            continue;
        }

        let Some((redacted_args, args_end)) = redact_command_args(input, command_end) else {
            out.push_str(&input[cursor..command_end]);
            cursor = command_end;
            continue;
        };

        out.push_str(&input[cursor..command_end]);
        out.push_str(&redacted_args);
        cursor = args_end;
    }

    out.push_str(&input[cursor..]);
    out
}

fn redact_command_args(input: &str, mut cursor: usize) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let mut replacement = String::new();
    let mut redacted_any = false;

    loop {
        let ws_start = cursor;
        while bytes.get(cursor).is_some_and(|b| b.is_ascii_whitespace()) {
            cursor += 1;
        }
        replacement.push_str(&input[ws_start..cursor]);
        if bytes.get(cursor) == Some(&b'*') {
            replacement.push('*');
            cursor += 1;
        }

        match bytes.get(cursor) {
            Some(b'[') => {
                let end = find_matching_delim(input, cursor, b'[', b']')?;
                replacement.push('[');
                replacement.push_str(BLACKOUT_TEX);
                replacement.push(']');
                cursor = end + 1;
                redacted_any = true;
            }
            Some(b'{') => {
                let end = find_matching_delim(input, cursor, b'{', b'}')?;
                replacement.push('{');
                replacement.push_str(BLACKOUT_TEX);
                replacement.push('}');
                cursor = end + 1;
                return Some((replacement, cursor));
            }
            _ => return redacted_any.then_some((replacement, cursor)),
        }
    }
}

fn find_matching_delim(input: &str, open_idx: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.get(open_idx) != Some(&open) {
        return None;
    }
    let mut depth = 0usize;
    let mut escaped = false;
    for (idx, b) in bytes.iter().enumerate().skip(open_idx) {
        if escaped {
            escaped = false;
            continue;
        }
        if *b == b'\\' {
            escaped = true;
            continue;
        }
        if *b == open {
            depth += 1;
        } else if *b == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

fn should_redact_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "tex"
                    | "ltx"
                    | "bib"
                    | "bbl"
                    | "sty"
                    | "cls"
                    | "clo"
                    | "cfg"
                    | "def"
                    | "dtx"
                    | "ins"
                    | "txt"
                    | "md"
            )
        })
        .unwrap_or(false)
}

fn path_to_archive_name(path: &Path) -> Result<String> {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(|s| s.to_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return Err(anyhow!("empty archive path"));
    }
    Ok(parts.join("/"))
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn redacted_source_filename(filename: &str, kind: SourceKind) -> String {
    let safe: String = filename
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    let safe = if safe.is_empty() {
        "source".to_string()
    } else {
        safe
    };
    let lower = safe.to_ascii_lowercase();
    match kind {
        SourceKind::SingleTex => suffix_before_ext(&safe, ".tex"),
        SourceKind::Zip => suffix_before_ext(&safe, ".zip"),
        SourceKind::TarGz => {
            if lower.ends_with(".tar.gz") {
                format!("{}-redacted.tar.gz", &safe[..safe.len() - ".tar.gz".len()])
            } else if lower.ends_with(".tgz") {
                format!("{}-redacted.tar.gz", &safe[..safe.len() - ".tgz".len()])
            } else {
                format!("{safe}-redacted.tar.gz")
            }
        }
        SourceKind::Unknown => format!("{safe}-redacted"),
    }
}

fn suffix_before_ext(filename: &str, ext: &str) -> String {
    if filename.to_ascii_lowercase().ends_with(ext) {
        format!(
            "{}-redacted{}",
            &filename[..filename.len() - ext.len()],
            ext
        )
    } else {
        format!("{filename}-redacted{ext}")
    }
}

/// Main entry. Compile a source upload into a PDF. The async-compatible
/// caller is responsible for storing the source archive + the resulting
/// PDF; we just return the bytes.
pub async fn compile(filename: &str, data: &[u8]) -> Result<Compiled, CompileError> {
    if data.is_empty() {
        return Err(CompileError::Empty);
    }
    let kind = detect_kind(filename, data);

    // Per-submission temp dir under the system tmp so concurrent
    // submissions are isolated.
    let tmp = tempfile::Builder::new()
        .prefix("prexiv-compile-")
        .tempdir()
        .map_err(|e| CompileError::Other(format!("creating temp dir: {e}")))?;
    let dir = tmp.path();

    match kind {
        SourceKind::SingleTex => {
            // Single .tex file; sanitize the filename, write into the
            // temp dir.
            let name = sanitize_filename(filename, ".tex");
            std::fs::write(dir.join(&name), data)
                .map_err(|e| CompileError::Other(format!("writing source: {e}")))?;
        }
        SourceKind::Zip => {
            extract_zip(data, dir).map_err(|e| CompileError::Extract(format!("{e}")))?;
        }
        SourceKind::TarGz => {
            extract_targz(data, dir).map_err(|e| CompileError::Extract(format!("{e}")))?;
        }
        SourceKind::Unknown => {
            return Err(CompileError::Other(format!(
                "unrecognised source format (expected .tex / .zip / .tar.gz); got filename '{filename}'"
            )));
        }
    }

    // Locate the main .tex file.
    let main = find_main_tex(dir).map_err(|e| match e {
        FindError::NoTex => CompileError::NoTexFiles,
        FindError::NoDocClass => CompileError::NoMainTex,
    })?;

    // Run latexmk if available; otherwise pdflatex twice for cross-refs.
    let result = timeout(TOTAL_TIMEOUT, compile_tex(dir, &main))
        .await
        .map_err(|_| CompileError::Timeout)??;
    Ok(result)
}

async fn compile_tex(workdir: &Path, main: &Path) -> Result<Compiled, CompileError> {
    let mainfile = main
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| CompileError::Other("main .tex has no filename".into()))?
        .to_string();

    // Prefer latexmk if installed (handles bibtex + re-runs); fall back
    // to two pdflatex passes.
    let used_latexmk = which("latexmk").is_some();
    let log = if used_latexmk {
        run_latexmk(workdir, &mainfile).await?
    } else {
        let _ = run_pdflatex(workdir, &mainfile).await; // first pass; ignore the error
        run_pdflatex(workdir, &mainfile).await? // second pass; final
    };

    // Look for the resulting PDF (same stem as the main).
    let stem = Path::new(&mainfile)
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| CompileError::Other("can't derive PDF name".into()))?;
    let pdf_path = workdir.join(format!("{stem}.pdf"));
    if !pdf_path.exists() {
        return Err(CompileError::NoPdf { log });
    }
    let pdf = std::fs::read(&pdf_path)
        .map_err(|e| CompileError::Other(format!("reading compiled PDF: {e}")))?;
    Ok(Compiled { pdf })
}

async fn run_pdflatex(workdir: &Path, mainfile: &str) -> Result<String, CompileError> {
    let out = timeout(
        PDFLATEX_TIMEOUT,
        Command::new("pdflatex")
            .arg("-interaction=nonstopmode")
            .arg("-no-shell-escape")
            .arg("-halt-on-error")
            .arg(mainfile)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await
    .map_err(|_| CompileError::Timeout)?
    .map_err(|e| CompileError::Other(format!("invoking pdflatex: {e}")))?;
    Ok(tail_log(&out.stdout, &out.stderr))
}

async fn run_latexmk(workdir: &Path, mainfile: &str) -> Result<String, CompileError> {
    let out = timeout(
        TOTAL_TIMEOUT,
        Command::new("latexmk")
            .arg("-pdf")
            .arg("-interaction=nonstopmode")
            .arg("-halt-on-error")
            .arg("-no-shell-escape")
            .arg(mainfile)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await
    .map_err(|_| CompileError::Timeout)?
    .map_err(|e| CompileError::Other(format!("invoking latexmk: {e}")))?;
    Ok(tail_log(&out.stdout, &out.stderr))
}

fn tail_log(stdout: &[u8], stderr: &[u8]) -> String {
    let combined: Vec<u8> = stdout
        .iter()
        .copied()
        .chain(b"\n--- stderr ---\n".iter().copied())
        .chain(stderr.iter().copied())
        .collect();
    let start = combined.len().saturating_sub(MAX_LOG_BYTES);
    String::from_utf8_lossy(&combined[start..]).to_string()
}

fn which(prog: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(prog))
            .find(|p| p.is_file())
    })
}

// ── archive extraction ──────────────────────────────────────────────

fn extract_zip(data: &[u8], dest: &Path) -> Result<()> {
    let reader = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).context("opening zip")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("reading zip entry")?;
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => continue, // path-traversal attempt — skip
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).context("mkdir")?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).context("mkdir parent")?;
        }
        let mut out = std::fs::File::create(&target).context("create file")?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).context("read zip entry")?;
        out.write_all(&buf).context("write extracted file")?;
    }
    Ok(())
}

fn extract_targz(data: &[u8], dest: &Path) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(data));
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries().context("opening tar")? {
        let mut entry = entry.context("reading tar entry")?;
        let rel = entry.path().context("entry path")?.into_owned();
        // Reject path traversal and links/special files.
        if !is_safe_relative_path(&rel) || !entry.header().entry_type().is_file() {
            continue;
        }
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).context("mkdir parent")?;
        }
        entry.unpack(&target).context("unpack")?;
    }
    Ok(())
}

// ── find main .tex ──────────────────────────────────────────────────

enum FindError {
    NoTex,
    NoDocClass,
}

fn find_main_tex(dir: &Path) -> Result<PathBuf, FindError> {
    let mut tex_files: Vec<PathBuf> = Vec::new();
    collect_tex_files(dir, &mut tex_files);
    if tex_files.is_empty() {
        return Err(FindError::NoTex);
    }
    if tex_files.len() == 1 {
        return Ok(tex_files.into_iter().next().unwrap());
    }
    // Pick the one containing \documentclass; prefer files literally
    // named main.tex / paper.tex.
    let preferred_names = ["main.tex", "paper.tex", "manuscript.tex"];
    for n in &preferred_names {
        if let Some(p) = tex_files.iter().find(|p| {
            p.file_name()
                .and_then(OsStr::to_str)
                .map(|s| s.eq_ignore_ascii_case(n))
                .unwrap_or(false)
        }) {
            return Ok(p.clone());
        }
    }
    for p in &tex_files {
        if let Ok(s) = std::fs::read_to_string(p) {
            if s.contains("\\documentclass") {
                return Ok(p.clone());
            }
        }
    }
    Err(FindError::NoDocClass)
}

fn collect_tex_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_tex_files(&p, out);
        } else if let Some(ext) = p.extension().and_then(OsStr::to_str) {
            if ext.eq_ignore_ascii_case("tex") {
                out.push(p);
            }
        }
    }
}

fn sanitize_filename(name: &str, default_ext: &str) -> String {
    let stem: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    if stem.is_empty() {
        format!("main{default_ext}")
    } else if !stem.to_ascii_lowercase().ends_with(default_ext) {
        format!("{stem}{default_ext}")
    } else {
        stem
    }
}

/// Async helper: write a byte slice to a file in the workdir without
/// blocking the runtime (useful if data is large).
#[allow(dead_code)]
async fn awrite(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut f = tokio::fs::File::create(path).await?;
    f.write_all(data).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> RedactionOptions {
        RedactionOptions {
            hide_human: true,
            hide_ai_model: true,
            human_name: Some("Alice Doe".to_string()),
            ai_models: vec!["Claude Opus 4.7".to_string(), "GPT-5".to_string()],
        }
    }

    #[test]
    fn private_single_tex_source_contains_blackout_not_private_terms() {
        let src = br"\documentclass{article}
\author{Alice Doe \\ 123 Example Street \\ University of Somewhere}
\begin{document}
Conducted with Claude Opus 4.7 and GPT-5.
\end{document}";
        let prepared = prepare_source("paper.tex", src, &opts()).expect("prepare");
        assert_eq!(prepared.filename, "paper-redacted.tex");
        let text = String::from_utf8(prepared.data).expect("utf8");
        assert!(text.contains(r"\author{\rule{6em}{1.1ex}}"));
        assert!(text.contains(BLACKOUT_TEX));
        assert!(!text.contains("Alice Doe"));
        assert!(!text.contains("123 Example Street"));
        assert!(!text.contains("Claude Opus 4.7"));
        assert!(!text.contains("GPT-5"));
    }

    #[test]
    fn private_zip_source_rewrites_text_entries_only() {
        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut zip_buf);
            let opts_zip = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("main.tex", opts_zip).expect("start tex");
            writer
                .write_all(br"\documentclass{article}\author{Alice Doe}\begin{document}GPT-5\end{document}")
                .expect("write tex");
            writer
                .start_file("figure.bin", opts_zip)
                .expect("start bin");
            writer.write_all(b"Alice Doe").expect("write bin");
            writer.finish().expect("finish zip");
        }

        let prepared =
            prepare_source("bundle.zip", &zip_buf.into_inner(), &opts()).expect("prepare");
        assert_eq!(prepared.filename, "bundle-redacted.zip");

        let mut archive = zip::ZipArchive::new(Cursor::new(prepared.data)).expect("zip");
        let mut main = String::new();
        archive
            .by_name("main.tex")
            .expect("main")
            .read_to_string(&mut main)
            .expect("read main");
        assert!(!main.contains("Alice Doe"));
        assert!(!main.contains("GPT-5"));
        assert!(main.contains(BLACKOUT_TEX));

        let mut bin = Vec::new();
        archive
            .by_name("figure.bin")
            .expect("bin")
            .read_to_end(&mut bin)
            .expect("read bin");
        assert_eq!(bin, b"Alice Doe");
    }
}
