use std::io::{self, IsTerminal};
use std::time::Instant;

use chrono::Local;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputTone {
    Standard,
    Progress,
    CacheHit,
    Success,
    Warning,
    Error,
    Muted,
    Header,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputStream {
    Stdout,
    Stderr,
}

pub struct ProgressIndicator {
    total: usize,
    completed: usize,
    failed: usize,
    start_time: Instant,
}

pub(crate) fn paint_stdout(text: impl AsRef<str>, tone: OutputTone) -> String {
    paint(text.as_ref(), tone, OutputStream::Stdout)
}

pub(crate) fn paint_stderr(text: impl AsRef<str>, tone: OutputTone) -> String {
    paint(text.as_ref(), tone, OutputStream::Stderr)
}

pub(crate) fn error_tag(tag: &str) -> String {
    paint_stderr(format!("error[{tag}]:"), OutputTone::Error)
}

pub(crate) fn warning_tag(tag: &str) -> String {
    paint_stderr(format!("warning[{tag}]:"), OutputTone::Warning)
}

pub(crate) fn success_text(text: impl AsRef<str>) -> String {
    paint_stdout(text, OutputTone::Success)
}

pub(crate) fn warning_text(text: impl AsRef<str>) -> String {
    paint_stderr(text, OutputTone::Warning)
}

pub(crate) fn error_text(text: impl AsRef<str>) -> String {
    paint_stderr(text, OutputTone::Error)
}

pub(crate) fn header_text(text: impl AsRef<str>) -> String {
    paint_stdout(text, OutputTone::Header)
}

pub(crate) fn standard_text(text: impl AsRef<str>) -> String {
    paint_stdout(text, OutputTone::Standard)
}

pub(crate) fn muted_text(text: impl AsRef<str>) -> String {
    paint_stdout(text, OutputTone::Muted)
}

pub fn print_timed_status(label: &str, name: &str) {
    let timestamp = Local::now().format("%H:%M:%S");
    println!(
        "{}: {} [{}]",
        paint_stdout(label, tone_for_label(label)),
        name,
        muted_text(timestamp.to_string())
    );
}

impl ProgressIndicator {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            completed: 0,
            failed: 0,
            start_time: Instant::now(),
        }
    }

    /// Start processing an item. Include estimated token count and timestamp when provided.
    pub fn start_item(&self, name: &str, estimated_tokens: Option<usize>) {
        self.start_item_with_label("Processing", name, estimated_tokens);
    }

    pub fn start_item_cached(&self, name: &str) {
        self.start_item_with_label("Cached", name, None);
    }

    pub fn start_item_up_to_date(&self, name: &str) {
        self.start_item_with_label("Up-to-date", name, None);
    }

    fn start_item_with_label(&self, label: &str, name: &str, estimated_tokens: Option<usize>) {
        let current = self.completed + self.failed + 1;
        let token_info = estimated_tokens
            .map(|n| format!(" {}", muted_text(format!("[~{n} tokens]"))))
            .unwrap_or_default();
        let timestamp = Local::now().format("%H:%M:%S");
        println!(
            "{}: {} ({}){} [{}]",
            paint_stdout(label, tone_for_label(label)),
            name,
            muted_text(format!("{current}/{}", self.total)),
            token_info,
            muted_text(timestamp.to_string())
        );
    }

    pub fn complete_item(&mut self, _name: &str, success: bool) {
        if success {
            self.completed += 1;
        } else {
            self.failed += 1;
        }
    }

    pub fn finish(&self) {
        let elapsed = self.start_time.elapsed();
        println!("\n{}", paint_stdout("=".repeat(60), OutputTone::Muted));
        println!("{}", header_text("Summary:"));
        println!("  {} {}", muted_text("Total:    "), self.total);
        println!(
            "  {} {}",
            paint_stdout("Succeeded:", OutputTone::Success),
            self.completed
        );
        println!(
            "  {} {}",
            paint_stdout("Failed:   ", OutputTone::Error),
            self.failed
        );
        println!(
            "  {} {:.2}s",
            muted_text("Duration: "),
            elapsed.as_secs_f64()
        );
        println!("{}", paint_stdout("=".repeat(60), OutputTone::Muted));
    }
}

fn paint(text: &str, tone: OutputTone, stream: OutputStream) -> String {
    if !colors_enabled(stream) {
        return text.to_string();
    }

    let code = match tone {
        OutputTone::Standard => "37",
        OutputTone::Progress => "34",
        OutputTone::CacheHit => "36",
        OutputTone::Success => "32",
        OutputTone::Warning => "33",
        OutputTone::Error => "31",
        OutputTone::Muted => "90",
        OutputTone::Header => "1;37",
    };

    format!("\u{001b}[{code}m{text}\u{001b}[0m")
}

fn colors_enabled(stream: OutputStream) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }

    if std::env::var_os("CLICOLOR_FORCE").is_some() {
        return true;
    }

    match stream {
        OutputStream::Stdout => io::stdout().is_terminal(),
        OutputStream::Stderr => io::stderr().is_terminal(),
    }
}

fn tone_for_label(label: &str) -> OutputTone {
    match label {
        "Processing" | "Submitting request" | "Received response" | "Still processing"
        | "Planning repair" | "Regenerating patch" => OutputTone::Progress,
        "Cached" | "Up-to-date" => OutputTone::CacheHit,
        _ => OutputTone::Standard,
    }
}
