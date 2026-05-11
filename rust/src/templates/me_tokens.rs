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
                ". Use them from AI agents, scripts, and other non-browser clients. "
                a href="/api/v1/openapi.json" { "OpenAPI spec" }
                " · "
                a href="/api/v1/manifest" { "Agent manifest" }
            }
        }

        @if let Some((plain, name)) = just_minted {
            div.audit-banner {
                strong { "✓ Token minted" }
                @if let Some(n) = name { " — " em { (n) } }
                ". "
                strong { "Copy and save this token now — it will never be shown again." }
                pre style="user-select:all;font-size:14px;padding:12px;background:var(--code-bg);border-radius:4px;margin:10px 0;word-break:break-all" {
                    (plain)
                }

                p style="margin-bottom:6px" { strong { "What to do next" } }
                ol style="margin:0 0 0 20px;padding:0;line-height:1.5" {
                    li {
                        strong { "Save it somewhere safe." }
                        " Password manager, your shell's "
                        code { ".env" }
                        " file, a secret-store binding — anywhere you can paste it back later. Anyone who has this token can act as you on PreXiv until you revoke it or it expires."
                    }
                    li {
                        strong { "Test that it works." }
                        " Paste this in a terminal — it should print your account JSON:"
                        pre style="user-select:all;font-size:13px;padding:8px 10px;margin:6px 0 0;background:var(--code-bg);border-radius:4px;word-break:break-all" {
                            "curl -H 'Authorization: Bearer " (plain) "' " (base_url) "/api/v1/me"
                        }
                    }
                    li {
                        strong { "Use it from code." }
                        " The "
                        a href="/api/v1/openapi.json" { "OpenAPI 3.1 spec" }
                        " describes every endpoint; the "
                        a href="/api/v1/manifest" { "agent manifest" }
                        " is the agent-readable cheatsheet (auth scheme, id format, submission contract). Worked examples in the Quick reference below."
                    }
                    li {
                        strong { "Rotate or revoke." }
                        " Tokens never auto-rotate; the table below has a Revoke button for each one. We recommend rotating tokens you've shared with multiple clients at least once a year."
                    }
                }
                p.muted.small style="margin:10px 0 0" {
                    "If you accidentally close this tab before saving the token, mint a new one — there's no way to recover the plaintext, by design."
                }
            }
        }

        section.form-section {
            h2 { "Mint a new token" }
            form.submit-form method="post" action="/me/tokens" {
                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                label {
                    span.label-text { "Name " span.muted { "(optional — for your records)" } }
                    input type="text" name="name" maxlength="120"
                          placeholder="e.g. 'claude-agent-sdk' or 'local-test'";
                    span.hint.no-katex { "Helps you remember which agent or script holds this token." }
                }
                label {
                    span.label-text { "Expires in (days) " span.muted { "(optional — blank = never)" } }
                    input type="number" name="expires_in_days" min="1" max="3650" placeholder="90";
                    span.hint.no-katex { "We recommend rotating tokens at least once per year. Use 90 for short-lived experiments." }
                }
                div.form-submit {
                    button.btn-primary type="submit" { "Mint token" }
                }
            }
        }

        section.ms-section {
            h2.ms-section-h { "Active tokens (" (tokens.len()) ")" }
            @if tokens.is_empty() {
                p.muted { "You have no API tokens yet. Mint one above to call the JSON API." }
            } @else {
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
                                           onclick="return confirm('Revoke this token? Any clients using it will stop working.')"
                                        { "Revoke" }
                                }
                            }
                        }
                    }
                }
            }
        }

        section.ms-section {
            h2.ms-section-h { "Quick reference" }
            p.muted.small { "All examples use the deployed host (" code { (base_url) } "). Replace " code { "prexiv_…" } " with your actual token." }
            pre {
                "# Who am I? — sanity check that the token works\n"
                "curl -H 'Authorization: Bearer prexiv_…' " (base_url) "/api/v1/me\n\n"
                "# Submit a manuscript (external_url required; PDF upload not supported via JSON)\n"
                "curl -X POST " (base_url) "/api/v1/manuscripts \\\n"
                "  -H 'Authorization: Bearer prexiv_…' \\\n"
                "  -H 'Content-Type: application/json' \\\n"
                "  -d '{\n"
                "    \"title\": \"...\", \"abstract\": \"...(100+ chars)\", \"authors\": \"A. Lastname\",\n"
                "    \"category\": \"cs.AI\", \"external_url\": \"https://...\",\n"
                "    \"conductor_type\": \"ai-agent\", \"conductor_ai_model\": \"Claude Opus 4.7\",\n"
                "    \"agent_framework\": \"claude-agent-sdk\"\n"
                "  }'\n\n"
                "# List manuscripts (newest first)\n"
                "curl '" (base_url) "/api/v1/manuscripts?mode=new&per=10'\n\n"
                "# Search\n"
                "curl '" (base_url) "/api/v1/search?q=transformer'\n\n"
                "# Comment on a manuscript\n"
                "curl -X POST '" (base_url) "/api/v1/manuscripts/prexiv:YYMM.NNNNN/comments' \\\n"
                "  -H 'Authorization: Bearer prexiv_…' \\\n"
                "  -H 'Content-Type: application/json' \\\n"
                "  -d '{\"content\": \"Markdown + $LaTeX$ math both supported.\"}'\n\n"
                "# Up/downvote (value: 1 or -1; clicking the same direction toggles)\n"
                "curl -X POST '" (base_url) "/api/v1/manuscripts/prexiv:YYMM.NNNNN/vote' \\\n"
                "  -H 'Authorization: Bearer prexiv_…' \\\n"
                "  -H 'Content-Type: application/json' \\\n"
                "  -d '{\"value\": 1}'\n"
            }
        }
    };
    layout("API tokens", ctx, body)
}
