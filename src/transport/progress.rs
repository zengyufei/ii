use anyhow::{Context, Result};
use std::{
    borrow::Cow,
    io::{IsTerminal, Write},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) struct TransferProgress {
    label: Cow<'static, str>,
    enabled: bool,
    line_mode: bool,
    draw_interval: Duration,
    total: Option<u64>,
    completed: u64,
    transferred: u64,
    started: std::time::Instant,
    last_draw: std::time::Instant,
    last_rate_completed: u64,
}

impl TransferProgress {
    pub(crate) fn new(
        label: &'static str,
        enabled: bool,
        total: Option<u64>,
        completed: u64,
    ) -> Self {
        let now = std::time::Instant::now();
        Self {
            label: Cow::Borrowed(label),
            enabled,
            line_mode: false,
            draw_interval: Duration::from_millis(250),
            total,
            completed,
            transferred: 0,
            started: now,
            last_draw: now,
            last_rate_completed: completed,
        }
    }

    pub(crate) fn multiline(
        label: String,
        enabled: bool,
        total: Option<u64>,
        completed: u64,
    ) -> Self {
        let now = std::time::Instant::now();
        Self {
            label: Cow::Owned(label),
            enabled,
            line_mode: true,
            draw_interval: Duration::from_secs(1),
            total,
            completed,
            transferred: 0,
            started: now,
            last_draw: now,
            last_rate_completed: completed,
        }
    }

    pub(crate) fn advance(&mut self, bytes: u64) {
        self.completed = self.completed.saturating_add(bytes);
        self.transferred = self.transferred.saturating_add(bytes);
        if self.enabled && self.last_draw.elapsed() >= self.draw_interval {
            self.draw(false);
        }
    }

    pub(crate) fn tick(&mut self) {
        if self.enabled && self.line_mode && self.last_draw.elapsed() >= self.draw_interval {
            self.draw(false);
        }
    }

    fn is_multiline(&self) -> bool {
        self.line_mode
    }

    pub(crate) fn finish(&mut self) {
        if self.enabled {
            self.draw(true);
            if !self.line_mode {
                eprintln!();
            }
        }
    }

    fn draw(&mut self, final_draw: bool) {
        let now = std::time::Instant::now();
        let elapsed = if final_draw {
            now.duration_since(self.started)
        } else {
            now.duration_since(self.last_draw)
        };
        let rate_bytes = if final_draw {
            self.transferred
        } else {
            self.completed.saturating_sub(self.last_rate_completed)
        };
        let bytes_per_second = if elapsed.as_secs_f64() > 0.0 {
            rate_bytes as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        let message = if final_draw {
            format!(
                "{}: done: {} in {} | avg {}/s",
                self.label,
                fmt_bytes(self.completed),
                fmt_duration(now.duration_since(self.started)),
                fmt_bytes(bytes_per_second as u64)
            )
        } else {
            match self.total {
                Some(total) if total > 0 => {
                    let pct = (self.completed.min(total) as f64 / total as f64) * 100.0;
                    format!(
                        "{}: {} / {} ({:.1}%) | {}/s",
                        self.label,
                        fmt_bytes(self.completed),
                        fmt_bytes(total),
                        pct,
                        fmt_bytes(bytes_per_second as u64)
                    )
                }
                _ => format!(
                    "{}: {} received | {}/s",
                    self.label,
                    fmt_bytes(self.completed),
                    fmt_bytes(bytes_per_second as u64)
                ),
            }
        };

        if self.line_mode {
            eprintln!("{message}");
        } else {
            eprint!("\r{message:<96}");
        }
        let _ = std::io::stderr().flush();
        self.last_draw = now;
        self.last_rate_completed = self.completed;
    }
}

pub(crate) fn should_show_progress(trace_enabled: bool) -> bool {
    std::io::stderr().is_terminal() && !trace_enabled
}

pub(crate) fn fmt_duration(duration: std::time::Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1_000 {
        format!("{ms}ms")
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

pub(crate) fn fmt_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.2} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.2} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.2} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) async fn copy_with_progress<R, W>(
    reader: &mut R,
    writer: &mut W,
    progress: &mut TransferProgress,
) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    if !progress.is_multiline() {
        loop {
            let n = reader.read(&mut buf).await.context("read payload")?;
            if n == 0 {
                return Ok(written);
            }
            writer.write_all(&buf[..n]).await.context("write payload")?;
            let n = n as u64;
            written = written.saturating_add(n);
            progress.advance(n);
        }
    }

    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.tick().await;
    loop {
        let read = reader.read(&mut buf);
        tokio::pin!(read);
        let n = loop {
            tokio::select! {
                result = &mut read => break result.context("read payload")?,
                _ = ticker.tick() => progress.tick(),
            }
        };
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await.context("write payload")?;
        let n = n as u64;
        written = written.saturating_add(n);
        progress.advance(n);
    }
    Ok(written)
}
