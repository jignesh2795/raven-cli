use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use raven_core::redact::{sanitize_text, truncate_utf8};
use raven_core::tool::{ApprovalRequest, Approver, Decision};

const MAX_DISPLAY_LINE_CHARS: usize = 400;

/// Presents a proposed change on stderr and asks the human to approve it.
///
/// The diff is always printed, including when `--yes` pre-approves the edit, so
/// the terminal still shows what changed. In a non-interactive run without
/// `--yes`, approval fails closed.
pub struct ConsoleApprover {
    auto: bool,
    color: bool,
    approve_all: AtomicBool,
}

impl ConsoleApprover {
    pub fn new(auto: bool, color_allowed: bool) -> Self {
        Self {
            auto,
            color: color_allowed && std::io::stderr().is_terminal(),
            approve_all: AtomicBool::new(false),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn render(&self, request: &ApprovalRequest, auto: bool) {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "\n{}", self.paint("1", &request.summary));
        for line in sanitize_text(&request.diff).lines() {
            let (line, _) = truncate_utf8(line, MAX_DISPLAY_LINE_CHARS);
            let painted = if line.starts_with("+++") || line.starts_with("---") {
                self.paint("1", line)
            } else if line.starts_with('+') {
                self.paint("32", line)
            } else if line.starts_with('-') {
                self.paint("31", line)
            } else if line.starts_with("@@") {
                self.paint("36", line)
            } else {
                line.to_string()
            };
            let _ = writeln!(stderr, "{painted}");
        }
        if auto {
            let _ = writeln!(
                stderr,
                "{}",
                self.paint("2", "(approved without prompting)")
            );
        }
    }

    fn ask(&self) -> bool {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            eprintln!(
                "raven: stdin is not a terminal, so the edit was not approved; re-run with --yes to approve edits unattended"
            );
            return false;
        }

        for _ in 0..3 {
            eprint!("Apply this change? [y]es / [n]o / [a]ll: ");
            let _ = std::io::stderr().flush();

            let mut line = String::new();
            match stdin.read_line(&mut line) {
                Ok(0) | Err(_) => return false, // EOF: fail closed
                Ok(_) => {}
            }

            match line.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => return true,
                "a" | "all" => {
                    self.approve_all.store(true, Ordering::Relaxed);
                    return true;
                }
                "" | "n" | "no" => return false,
                _ => eprintln!("  answer y, n, or a"),
            }
        }
        false
    }
}

#[async_trait]
impl Approver for ConsoleApprover {
    async fn approve(&self, request: ApprovalRequest) -> Decision {
        let auto = self.auto || self.approve_all.load(Ordering::Relaxed);
        self.render(&request, auto);
        if auto {
            return Decision::Approved;
        }
        if self.ask() {
            Decision::Approved
        } else {
            Decision::Denied("declined at the approval prompt".into())
        }
    }
}
