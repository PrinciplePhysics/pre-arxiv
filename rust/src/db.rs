use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use sqlx::postgres::PgPoolOptions;

pub type Db = sqlx::Postgres;
pub type DbPool = sqlx::PgPool;

static SQL_CACHE: LazyLock<Mutex<HashMap<&'static str, &'static str>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn connect(database_url: &str) -> anyhow::Result<DbPool> {
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Convert old SQLite-style `?` bind markers to PostgreSQL `$1`, `$2`, ...
/// for static SQL literals. This keeps the migration contained while the
/// application moves to PostgreSQL. The converted SQL is leaked once per
/// distinct static string and reused through SQLx's statement cache.
pub fn pg(sql: &'static str) -> &'static str {
    if !sql.contains('?') {
        return sql;
    }
    let mut cache = SQL_CACHE.lock().expect("SQL_CACHE poisoned");
    if let Some(converted) = cache.get(sql) {
        return converted;
    }
    let converted = rewrite_placeholders(sql);
    let leaked: &'static str = Box::leak(converted.into_boxed_str());
    cache.insert(sql, leaked);
    leaked
}

/// Convert bind markers in a dynamically assembled SQL string.
pub fn pg_dynamic(sql: &str) -> String {
    rewrite_placeholders(sql)
}

fn rewrite_placeholders(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 1usize;
    let mut in_single_quote = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            out.push(c);
            if in_single_quote && chars.peek() == Some(&'\'') {
                out.push(chars.next().unwrap());
                continue;
            }
            in_single_quote = !in_single_quote;
            continue;
        }
        if c == '?' && !in_single_quote {
            out.push('$');
            out.push_str(&n.to_string());
            n += 1;
        } else {
            out.push(c);
        }
    }
    out
}
