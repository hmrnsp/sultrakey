//! `list`: key names and their status. Never a value. Needs no key file.

use anyhow::Result;

use super::{Ctx, load_env};
use crate::output;

pub fn run(ctx: &Ctx) -> Result<i32> {
    let doc = load_env(ctx)?;
    if doc.header.is_none() {
        output::warn(&format!(
            "{} belum dikelola sultrakey (tidak ada baris SULTRAKEY_APP).",
            ctx.env.display()
        ));
    }
    let rows: Vec<(&str, &str)> = doc
        .entries()
        .map(|entry| (entry.key.as_str(), entry.status().label()))
        .collect();
    if rows.is_empty() {
        output::info("Tidak ada key.");
        return Ok(0);
    }
    let width = rows
        .iter()
        .map(|(key, _)| key.len())
        .max()
        .unwrap_or(0)
        .max(3);
    let mut text = format!("{:<width$}  STATUS\n", "KEY");
    for (key, status) in rows {
        text.push_str(&format!("{key:<width$}  {status}\n"));
    }
    output::print(&text)?;
    Ok(0)
}
