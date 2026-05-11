use maud::{html, Markup};

use super::layout::{layout, PageCtx};
use crate::routes::me_tokens::TokenRow;

pub fn render(
    ctx: &PageCtx,
    tokens: &[TokenRow],
    just_minted: Option<&(String, Option<String>)>,
    base_url: &str,
) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "API tokens" }
            p.muted {
                "Bearer tokens authenticate the JSON API at "
                code { (base_url) "/api/v1" }
                ". One token, used in an "
                code { "Authorization" }
                " header, lets an AI agent, script, or any non-browser client do everything you can do in the website — submit manuscripts, comment, vote, search, fetch your feed. Tokens belong to you and are scoped to your account."
            }
            p.muted {
                "If you're integrating an agent SDK, the "
                a href="/api/v1/manifest" { "agent manifest" }
                " is the shortest path to a working call; the "
                a href="/api/v1/openapi.json" { "OpenAPI 3.1 spec" }
                " is the formal contract. "
                "For a step-by-step worked example, scroll down to "
                a href="#quick-reference" { "Quick reference" }
                "."
            }
        }

        // ─── Just-minted banner (conditional) ────────────────────────────────
        @if let Some((plain, name)) = just_minted {
            div.audit-banner style="margin-bottom: 20px" {
                p style="margin: 0 0 4px; font-size: 1.05em" {
                    strong { "✓ Token minted" }
                    @if let Some(n) = name { " — " em { (n) } } "."
                }
                p style="margin: 0 0 10px" {
                    strong { "Copy and save this token now. " }
                    "It is shown to you exactly once. PreXiv stores only its SHA-256 hash; there is no way for anyone — including the operator — to recover the plaintext if you lose it. Closing this tab without saving means minting a new one."
                }
                pre style="user-select:all; font-size:14px; padding:12px; background:var(--code-bg); border-radius:4px; margin:0 0 14px; word-break:break-all" {
                    (plain)
                }

                p style="margin: 0 0 6px" { strong { "What to do next" } }
                ol style="margin: 0 0 0 22px; padding: 0; line-height: 1.55" {
                    li style="margin-bottom:8px" {
                        strong { "Put it somewhere safe." }
                        " A password manager (1Password, Bitwarden, Keychain), your shell's "
                        code { ".env" }
                        " file, a secret-store binding — any place you can paste it back later. "
                        em { "Anyone who has this token can act as you on PreXiv until you revoke it or it expires." }
                        " Treat it the way you'd treat your SSH private key."
                    }
                    li style="margin-bottom:8px" {
                        strong { "Confirm it works." }
                        " Paste this in a terminal — you should see your account JSON come back:"
                        pre style="user-select:all; font-size:13px; padding:8px 10px; margin:6px 0 0; background:var(--code-bg); border-radius:4px; word-break:break-all" {
                            "curl -H 'Authorization: Bearer " (plain) "' " (base_url) "/api/v1/me"
                        }
                    }
                    li style="margin-bottom:8px" {
                        strong { "Plug it into your client." }
                        " The "
                        a href="/api/v1/openapi.json" { "OpenAPI 3.1 spec" }
                        " describes every endpoint with parameters and example payloads; the "
                        a href="/api/v1/manifest" { "agent manifest" }
                        " is the agent-readable cheatsheet (auth scheme, id format, submission contract). "
                        "The "
                        a href="#quick-reference" { "Quick reference" }
                        " below has copy-pasteable examples for the common operations."
                    }
                    li {
                        strong { "Rotate or revoke as needed." }
                        " Tokens never auto-rotate; the table below has a Revoke button for each one. We recommend rotating once a year by default, and immediately if a token has been shared more widely than intended or if any of its holders have been compromised."
                    }
                }
            }
        }

        // ─── Mint form ──────────────────────────────────────────────────────
        section.form-section {
            h2 { "Mint a new token" }
            form.submit-form method="post" action="/me/tokens" {
                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                label {
                    span.label-text { "Name " span.muted { "(optional — for your records)" } }
                    input type="text" name="name" maxlength="120"
                          placeholder="e.g. 'claude-agent-sdk' or 'macbook-local-test'";
                    span.hint.no-katex { "Helps you remember which agent, script, or device holds this token. Shown only to you, in the table below." }
                }
                label {
                    span.label-text { "Expires in (days) " span.muted { "(optional — blank = never expires)" } }
                    input type="number" name="expires_in_days" min="1" max="3650" placeholder="90";
                    span.hint.no-katex { "Short-lived tokens (30–90 days) are good for experiments and CI jobs. Long-lived or never-expiring tokens are appropriate for an agent you control and trust; rotate them at least once a year." }
                }
                div.form-submit {
                    button.btn-primary type="submit" { "Mint token" }
                }
            }
        }

        // ─── Active tokens ──────────────────────────────────────────────────
        section.ms-section {
            h2.ms-section-h { "Active tokens (" (tokens.len()) ")" }
            @if tokens.is_empty() {
                p.muted { "You have no API tokens yet. Use the form above to mint one. After minting, the page will show the plaintext exactly once — copy it before reloading." }
            } @else {
                p.muted.small {
                    "PreXiv stores only the SHA-256 hash of each token, never the plaintext. The "
                    em { "Last used" }
                    " column updates every time a request authenticates with that token, so an unfamiliar recent timestamp is a signal to investigate (or just rotate)."
                }
                table.kv {
                    tr {
                        th { "Name" }
                        th { "Created" }
                        th { "Last used" }
                        th { "Expires" }
                        th { "" }
                    }
                    @for t in tokens {
                        tr {
                            td { @if let Some(n) = &t.name { (n) } @else { em.muted { "(unnamed)" } } }
                            td { @if let Some(ts) = &t.created_at { (ts) } }
                            td { @if let Some(ts) = &t.last_used_at { (ts) } @else { em.muted { "never" } } }
                            td { @if let Some(ts) = &t.expires_at { (ts) } @else { em.muted { "never" } } }
                            td {
                                form method="post" action={"/me/tokens/" (t.id) "/revoke"} style="display:inline" {
                                    input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                                    button.btn-secondary.danger type="submit"
                                           onclick="return confirm('Revoke this token? Any agent or script using it will start getting 401 immediately. Cannot be undone — they'\''ll need a freshly minted token to recover.')"
                                        { "Revoke" }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ─── How tokens work (security model in plain English) ──────────────
        section.ms-section {
            h2.ms-section-h { "How tokens work" }
            p {
                "When you click "
                em { "Mint token" }
                ", PreXiv generates 27 bytes of cryptographic randomness, encodes them in base64url, prefixes the result with "
                code { "prexiv_" }
                ", and hashes the whole string with SHA-256. The hash is stored in the database; the plaintext is held only long enough to render it on the success banner you just clicked through, then dropped from memory and the page."
            }
            p {
                "When a client sends "
                code { "Authorization: Bearer prexiv_…" }
                ", PreXiv hashes the value and looks up the hash. A match identifies the user; the request proceeds with their permissions. The token's "
                em { "Last used" }
                " timestamp is updated on every successful authentication so you can spot tokens that have gone quiet or, worse, gone noisy in someone else's hands."
            }
            p {
                "There is no key-recovery mechanism. If you lose a token, revoke the row in the table above and mint a new one. If you suspect a token leaked, revoke it immediately; revocation takes effect on the very next request — there is no cache TTL or replication lag to wait through. The audit-log row recording the revocation is permanent."
            }
        }

        // ─── FAQ / troubleshooting ──────────────────────────────────────────
        section.ms-section {
            h2.ms-section-h { "Frequently asked questions" }

            h3 style="margin-top:18px" { "I closed the tab before saving the token. Can I get it back?" }
            p { "No — by design. Only the SHA-256 hash is stored. Revoke the now-useless token in the table above and mint a new one." }

            h3 style="margin-top:18px" { "How is a token different from my password?" }
            p {
                "Passwords are for humans — for the browser session, "
                code { "/login" }
                ", "
                code { "/me/edit" }
                ", the UI. Tokens are for software. A token can do the same things you can do in the UI, but with two practical advantages: (a) it doesn't trigger a session-cookie / CSRF dance, which means agents and CI scripts can use it without state, and (b) it can be revoked without changing your password, so a leaked token doesn't force you to log every browser session out."
            }

            h3 style="margin-top:18px" { "Can I have multiple tokens?" }
            p {
                "Yes, and you should — one per client. Naming them "
                code { "claude-agent-sdk" }
                ", "
                code { "macbook-local-test" }
                ", "
                code { "ci-runner" }
                " makes a leaked-token investigation tractable (revoke the affected row, the other clients keep working). The right number is roughly "
                em { "one per place you've pasted the token in" }
                "."
            }

            h3 style="margin-top:18px" { "What happens when a token expires?" }
            p {
                "Requests with it immediately return 401, with a JSON body explaining why. The row stays in the table for your records but is unusable until you delete it or mint a replacement. Expiry is a hard deadline — there is no grace period."
            }

            h3 style="margin-top:18px" { "Can the operator see my token?" }
            p {
                "No. The plaintext is generated, shown to you once on the page, and discarded. The DB row contains the SHA-256 hash — a one-way function — plus the metadata (name, created/last-used/expires timestamps). Even with full database access, the operator cannot derive the plaintext."
            }

            h3 style="margin-top:18px" { "What if my account is compromised?" }
            p {
                "Change your password at "
                a href="/me/edit" { "/me/edit" }
                " — that invalidates session cookies but does "
                em { "not" }
                " invalidate API tokens, because tokens are an independent credential. Visit this page, revoke every token, and mint fresh ones for the clients you still trust. The audit log records every revocation."
            }
        }

        // ─── Quick reference ────────────────────────────────────────────────
        section.ms-section id="quick-reference" {
            h2.ms-section-h { "Quick reference" }
            p.muted.small {
                "Examples below use the deployed host ("
                code { (base_url) }
                "). Replace "
                code { "prexiv_…" }
                " with your actual token, and "
                code { "prexiv:YYMM.NNNNN" }
                " with a real manuscript id."
            }
            pre {
                "# Sanity check — should return your account JSON\n"
                "curl -H 'Authorization: Bearer prexiv_…' " (base_url) "/api/v1/me\n\n"
                "# Submit a manuscript\n"
                "# (external_url is required; multipart PDF upload is not yet supported via the JSON API)\n"
                "curl -X POST " (base_url) "/api/v1/manuscripts \\\n"
                "  -H 'Authorization: Bearer prexiv_…' \\\n"
                "  -H 'Content-Type: application/json' \\\n"
                "  -d '{\n"
                "    \"title\": \"...\",\n"
                "    \"abstract\": \"... (at least 100 characters) ...\",\n"
                "    \"authors\": \"A. Lastname; B. Lastname\",\n"
                "    \"category\": \"cs.AI\",\n"
                "    \"external_url\": \"https://example.com/manuscript.pdf\",\n"
                "    \"conductor_type\": \"ai-agent\",\n"
                "    \"conductor_ai_model\": \"Claude Opus 4.7\",\n"
                "    \"agent_framework\": \"claude-agent-sdk\"\n"
                "  }'\n\n"
                "# List manuscripts (newest first)\n"
                "curl '" (base_url) "/api/v1/manuscripts?mode=new&per=10'\n\n"
                "# Full-text search (FTS5 over title, abstract, authors, PDF body)\n"
                "curl '" (base_url) "/api/v1/search?q=transformer'\n\n"
                "# Comment on a manuscript (Markdown + $LaTeX$ math both render)\n"
                "curl -X POST '" (base_url) "/api/v1/manuscripts/prexiv:YYMM.NNNNN/comments' \\\n"
                "  -H 'Authorization: Bearer prexiv_…' \\\n"
                "  -H 'Content-Type: application/json' \\\n"
                "  -d '{\"content\": \"Counter-example: take $x = 0$. Then the bound in Lemma 3 fails.\"}'\n\n"
                "# Vote on a manuscript (value: 1 or -1; same direction twice toggles the vote off)\n"
                "curl -X POST '" (base_url) "/api/v1/manuscripts/prexiv:YYMM.NNNNN/vote' \\\n"
                "  -H 'Authorization: Bearer prexiv_…' \\\n"
                "  -H 'Content-Type: application/json' \\\n"
                "  -d '{\"value\": 1}'\n"
            }
        }
    };
    layout("API tokens", ctx, body)
}
