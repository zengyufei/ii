use anyhow::{Context, Result, bail};
use std::io::{BufRead, Write};

pub(crate) fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush().context("flush prompt")?;
    let mut input = String::new();
    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    locked.read_line(&mut input).context("read prompt")?;
    let value = input.trim().to_string();
    if value.is_empty() {
        bail!(
            "empty value is not allowed for {}",
            prompt.trim_end_matches(": ")
        );
    }
    Ok(value)
}

pub(crate) fn prompt_optional_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush().context("flush prompt")?;
    let mut input = String::new();
    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    locked.read_line(&mut input).context("read prompt")?;
    Ok(input.trim().to_string())
}
