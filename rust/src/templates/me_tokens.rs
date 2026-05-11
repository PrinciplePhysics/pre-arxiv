use maud::{html, Markup};

use super::layout::{layout, PageCtx};
use crate::routes::me_tokens::TokenRow;

pub fn render(
    ctx: &PageCtx,
    tokens: &[TokenRow],
    just_minted: Option<&(String, Option<String>)>,
) -> Markup {
    let body = html! {
        div.page-header {
            h1 { "API tokens" }
            p.muted {
                "Bearer tokens authenticate the JSON API at "
                code { "/api/v1" }
                ". Use them from AI agents, scripts, and other non-browser clients. "
                a href="/api/v1/openapi.json" { "OpenAPI spec" }
                " · "
                a href="/api/v1/manifest" { "Agent manifest" }
            }
        }

        @if let Some((plain, name)) = just_minted {
            div.audit-banner {
                strong { "Token minted" }
                @if let Some(n) = name { " (" (n) ")" }
                ". Copy it now — it will never be shown again:"
                pre style="user-select:all;font-size:14px;padding:12px;background:var(--code-bg);border-radius:4px;margin-top:8px" {
                    (plain)
                }
                p.muted.small {
                    "Try it: "
                    code { "curl -H 'Authorization: Bearer " (plain) "' http://localhost:3001/api/v1/me" }
                }
            }
        }

        section.ms-section {
            h2.ms-section-h { "Mint a new token" }
            form method="post" action="/me/tokens" {
                input type="hidden" name="csrf_token" value=(ctx.csrf_token);
                label {
                    "Name (optional — for your records)"
                    input type="text" name="name" placeholder="e.g. 'claude-agent-sdk' or 'local-test'";
                }
                label {
                    "Expires in (days, optional — blank = never)"
                    input type="number" name="expires_in_days" min="1" max="3650" placeholder="90";
                }
                button.btn-primary type="submit" { "Mint token" }
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
            pre {
                "# Who am I?\n"
                "curl -H 'Authorization: Bearer prexiv_…' http://localhost:3001/api/v1/me\n\n"
                "# Submit a manuscript (external_url required; PDF upload not supported via JSON)\n"
                "curl -X POST http://localhost:3001/api/v1/manuscripts \\\n"
                "  -H 'Authorization: Bearer prexiv_…' \\\n"
                "  -H 'Content-Type: application/json' \\\n"
                "  -d '{\n"
                "    \"title\": \"...\", \"abstract\": \"...(100+ chars)\", \"authors\": \"A. Lastname\",\n"
                "    \"category\": \"cs.AI\", \"external_url\": \"https://...\",\n"
                "    \"conductor_type\": \"ai-agent\", \"conductor_ai_model\": \"Claude Opus 4.7\",\n"
                "    \"agent_framework\": \"claude-agent-sdk\"\n"
                "  }'\n\n"
                "# List manuscripts (newest first)\n"
                "curl 'http://localhost:3001/api/v1/manuscripts?mode=new&per=10'\n\n"
                "# Search\n"
                "curl 'http://localhost:3001/api/v1/search?q=transformer'\n"
            }
        }
    };
    layout("API tokens", ctx, body)
}
