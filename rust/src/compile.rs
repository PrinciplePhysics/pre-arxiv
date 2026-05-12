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
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const PDFLATEX_TIMEOUT: Duration = Duration::from_secs(60);
const TOTAL_TIMEOUT:    Duration = Duration::from_secs(120);
const MAX_LOG_BYTES:    usize = 32 * 1024;

#[derive(Debug)]
pub struct Compiled {
    pub pdf: Vec<u8>,
    pub log: String,
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

/// Detect what kind of upload `data` is, by looking at its filename
/// extension (and as a tie-breaker, the magic bytes for zip / gzip).
pub fn detect_kind(filename: &str, data: &[u8]) -> SourceKind {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".tex") {
        SourceKind::SingleTex
    } else if lower.ends_with(".zip") || data.starts_with(b"PK\x03\x04") {
        SourceKind::Zip
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || data.starts_with(&[0x1f, 0x8b]) {
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
        run_pdflatex(workdir, &mainfile).await?         // second pass; final
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
    Ok(Compiled { pdf, log })
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
        // Reject path traversal.
        if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
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

enum FindError { NoTex, NoDocClass }

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
            p.file_name().and_then(OsStr::to_str).map(|s| s.eq_ignore_ascii_case(n)).unwrap_or(false)
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
