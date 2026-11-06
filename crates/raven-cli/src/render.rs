use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};

use raven_core::agent::Observer;
use raven_core::redact::{sanitize_text, truncate_utf8, Redactor};
use serde_json::{json, Value};

pub struct ConsoleObserver {
    json: bool,
    color: bool,
    redactor: Arc<Redactor>,
    line_start: Mutex<bool>,
}

impl ConsoleObserver {
    pub fn new(json: bool, color_allowed: bool, redactor: Arc<Redactor>) -> Self {
        let color = color_allowed && std::io::stdout().is_terminal();
        Self {
            json,
            color,
            redactor,
            line_start: Mutex::new(true),
        }
    }

    fn ensure_newline(&self) {
        if self.json {
            return;
        }
        let mut line_start = self
            .line_start
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !*line_start {
            println!();
            *line_start = true;
        }
    }

    fn mark_line_end(&self, text: &str) {
        if self.json {
            return;
        }
        if let Ok(mut line_start) = self.line_start.lock() {
            *line_start = text.ends_with('\n');
        }
    }

    fn dim(&self, text: &str) -> String {
        if self.color {
            format!("\x1b[2m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn green(&self, text: &str) -> String {
        if self.color {
            format!("\x1b[32m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn red(&self, text: &str) -> String {
        if self.color {
            format!("\x1b[31m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

impl Observer for ConsoleObserver {
    fn on_text(&self, text: &str) {
        let text = sanitize_text(text);
        if self.json {
            emit(json!({ "v": 1, "type": "text_delta", "text": text }));
            return;
        }
        print!("{text}");
        let _ = std::io::stdout().flush();
        self.mark_line_end(&text);
    }

    fn on_tool_call(&self, name: &str, args: &Value) {
        let name = self.redactor.redact(&sanitize_text(name));
        let mut args = args.clone();
        self.redactor.redact_json(&mut args);
        let rendered = serde_json::to_string(&args).unwrap_or_else(|_| "{}".into());
        let (rendered, _) = truncate_utf8(&rendered, 200);

        if self.json {
            emit(json!({
                "v": 1,
                "type": "tool_call",
                "name": name,
                "arguments": args,
            }));
            return;
        }

        self.ensure_newline();
        println!("{}", self.dim(&format!("→ {name}({rendered})")));
    }

    fn on_tool_result(&self, name: &str, ok: bool, summary: &str) {
        let name = sanitize_text(name);
        let summary = self.redactor.redact(&sanitize_text(summary));
        if self.json {
            emit(json!({
                "v": 1,
                "type": "tool_result",
                "name": name,
                "ok": ok,
                "summary": summary,
            }));
            return;
        }

        self.ensure_newline();
        let marker = if ok {
            self.green("✓")
        } else {
            self.red("✗")
        };
        println!("  {marker} {}: {}", name, self.dim(&summary));
    }

    fn on_notice(&self, message: &str) {
        let message = self.redactor.redact(&sanitize_text(message));
        if self.json {
            emit(json!({ "v": 1, "type": "notice", "message": message }));
            return;
        }
        eprintln!("{}", self.dim(&format!("· {message}")));
    }
}

pub fn emit(value: Value) {
    if let Ok(line) = serde_json::to_string(&value) {
        println!("{line}");
        let _ = std::io::stdout().flush();
    }
}
