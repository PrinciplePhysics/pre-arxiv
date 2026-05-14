//! Public-PDF watermarking.
//!
//! PreXiv stores only the public PDF artifact. For LaTeX submissions this is
//! the compiled PDF; for direct-PDF submissions this is the uploaded PDF after
//! stamping. The original direct PDF is never persisted.

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tokio::process::Command;
use tokio::time::timeout;

const WATERMARK_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn watermark_pdf(
    input: &[u8],
    manuscript_id: &str,
    category: &str,
    app_url: &str,
) -> Result<Vec<u8>> {
    if input.is_empty() {
        bail!("PDF is empty");
    }
    if !input.starts_with(b"%PDF-") {
        bail!("input is not a PDF");
    }

    let tmp = tempfile::tempdir().context("creating watermark tempdir")?;
    let input_path = tmp.path().join("input.pdf");
    let output_path = tmp.path().join("output.pdf");
    let ps_path = tmp.path().join("prexiv-watermark.ps");

    tokio::fs::write(&input_path, input)
        .await
        .context("writing watermark input")?;
    tokio::fs::write(
        &ps_path,
        watermark_postscript(manuscript_id, category, app_url),
    )
    .await
    .context("writing watermark postscript")?;

    run_ghostscript(&input_path, &ps_path, &output_path).await?;

    let output = tokio::fs::read(&output_path)
        .await
        .context("reading watermarked PDF")?;
    if !output.starts_with(b"%PDF-") {
        bail!("watermark output is not a PDF");
    }
    Ok(output)
}

async fn run_ghostscript(input_path: &Path, ps_path: &Path, output_path: &Path) -> Result<()> {
    let gs = std::env::var("PREXIV_GHOSTSCRIPT_BIN").unwrap_or_else(|_| "gs".to_string());
    let out_arg = format!("-sOutputFile={}", output_path.display());

    let child = Command::new(&gs)
        .arg("-q")
        .arg("-dNOPAUSE")
        .arg("-dBATCH")
        .arg("-dSAFER")
        .arg("-dAutoRotatePages=/None")
        .arg("-sDEVICE=pdfwrite")
        .arg("-dCompatibilityLevel=1.7")
        .arg(out_arg)
        .arg(ps_path)
        .arg(input_path)
        .output();

    let output = timeout(WATERMARK_TIMEOUT, child)
        .await
        .map_err(|_| anyhow!("Ghostscript watermarking timed out"))?
        .with_context(|| format!("running Ghostscript binary '{gs}'"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "Ghostscript watermarking failed: {}{}{}",
            stderr.trim(),
            if stderr.trim().is_empty() || stdout.trim().is_empty() {
                ""
            } else {
                " "
            },
            stdout.trim()
        );
    }
    Ok(())
}

fn watermark_postscript(manuscript_id: &str, category: &str, app_url: &str) -> String {
    let base = app_url.trim_end_matches('/');
    let today = chrono::Utc::now().format("%d %b %Y");
    let display_id = manuscript_id
        .strip_prefix("prexiv:")
        .unwrap_or(manuscript_id);
    let label = format!("PreXiv:{display_id} [{category}] {today}");
    let href = format!("{base}/m/{manuscript_id}");
    format!(
        r#"/PreXivWatermark ({}) def
	/PreXivHref ({}) def
	<< /EndPage {{
	  /PreXivEndReason exch def
	  /PreXivPageCount exch def
	  PreXivEndReason 2 eq {{ false }} {{
	    PreXivPageCount 0 eq {{
	      /PreXivFont /TimesNewRomanPSMT findfont 14 scalefont def
	      PreXivFont setfont
	      clippath pathbbox
	      /PreXivPageTop exch def
	      pop
	      /PreXivPageBottom exch def
	      pop
	      /PreXivTextWidth PreXivWatermark stringwidth pop def
	      /PreXivX 36 def
	      /PreXivY PreXivPageBottom PreXivPageTop PreXivPageBottom sub PreXivTextWidth sub 2 div add def
	      gsave
	        PreXivFont setfont
	        0.55 setgray
	        PreXivX PreXivY translate
	        90 rotate
	        0 0 moveto
	        PreXivWatermark show
	      grestore
	      [ /Rect [ PreXivX 10 sub PreXivY 4 sub PreXivX 12 add PreXivY PreXivTextWidth add 4 add ]
	        /Border [ 0 0 0 ]
	        /Action << /Subtype /URI /URI PreXivHref >>
	        /Subtype /Link
	        /ANN pdfmark
	    }} if
	    true
	  }} ifelse
	}} bind >> setpagedevice
	"#,
        ps_string(&label),
        ps_string(&href)
    )
}

fn ps_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str(r"\\"),
            '(' => out.push_str(r"\("),
            ')' => out.push_str(r"\)"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ps_string, watermark_postscript};

    #[test]
    fn postscript_string_escape_covers_delimiters() {
        assert_eq!(ps_string(r"a\b(c)d"), r"a\\b\(c\)d");
    }

    #[test]
    fn postscript_stamps_first_page_in_times_new_roman() {
        let ps = watermark_postscript("prexiv:260513.3n9jxa", "cs.AI", "https://prexiv.example/");

        assert!(ps.contains("PreXivPageCount 0 eq"));
        assert!(ps.contains("/TimesNewRomanPSMT findfont 14 scalefont def"));
        assert!(ps.contains("PreXivPageTop PreXivPageBottom sub PreXivTextWidth sub 2 div"));
        assert!(!ps.contains("/Helvetica findfont"));
    }
}
