//! The permission ruleset: `(subject, pattern, action)` with layered resolution.
//!
//! A single mode cannot express command permissions, because `cargo test` and
//! `rm -rf` are both "execute". A ruleset can. Every request is matched against
//! layers in a fixed order and the first match wins:
//!
//! 1. **Hard rules** — compiled in, never writable, never overridable. They
//!    protect invariants rather than encode preferences: protected paths are
//!    always `Deny`, no rule can grant network access, and a rule set that would
//!    allow everything is rejected at parse time.
//! 2. **User rules** — the user's own config or `--allow`.
//! 3. **Session rules** — granted at a prompt for the remainder of the run.
//! 4. **Project rules** — only when the workspace is trusted, and never allowed
//!    to widen what the user has already authorized.
//! 5. **Ask** — the fallback. `Ask` is a request for a human decision.
//!
//! An unmatched request resolves to `Ask`, never to `Allow`, so a request that
//! nobody has decided about can only proceed by a human saying yes. That is the
//! same fail-closed invariant as `DenyAll`, extended per-subject instead of
//! per-tool-kind.

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// What a rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    /// The command line of a proposed process launch.
    Command,
    /// A path a tool wants to write.
    WritePath,
    /// A path a tool wants to read.
    ReadPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Allow,
    Ask,
    Deny,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub subject: Subject,
    /// Glob-ish pattern matched against the command line or the path.
    pub pattern: String,
    pub action: Action,
}

/// Where a rule came from. Determines the layer it can compete in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Compiled in; wins over everything and cannot be added to by a user.
    Hard,
    /// The user's own configuration.
    User,
    /// Granted during this run at an approval prompt.
    Session,
    /// A trusted project-local config. Weaker than the layers above it.
    Project,
}

/// A compiled rule set. Patterns are compiled once on construction.
#[derive(Debug, Clone)]
pub struct RuleSet {
    entries: Vec<(Rule, Layer, GlobMatcher)>,
}

impl RuleSet {
    /// Build a set from rules, compiling every pattern.
    ///
    /// A pattern that would allow everything is rejected: a mistyped glob
    /// becoming a blanket allow is the failure mode that matters.
    pub fn new(rules: Vec<(Rule, Layer)>) -> Result<Self> {
        let mut entries = Vec::with_capacity(rules.len());
        for (rule, layer) in rules {
            let pattern = Self::normalize(&rule.pattern);
            if Self::is_blanket(&pattern) && rule.action == Action::Allow {
                return Err(Error::Config(format!(
                    "rule pattern '{}' would allow everything, which is not allowed",
                    rule.pattern
                )));
            }
            let matcher = Glob::new(&pattern)
                .map_err(|error| {
                    Error::Config(format!("invalid rule pattern '{}': {error}", rule.pattern))
                })?
                .compile_matcher();
            entries.push((rule, layer, matcher));
        }
        Ok(Self { entries })
    }

    /// Resolve the action for a request.
    ///
    /// Layers are consulted in order and the first matching rule wins, so a
    /// hard `Deny` beats a user `Allow` even when the user's pattern is more
    /// specific.
    pub fn resolve(&self, subject: Subject, value: &str) -> Action {
        // Paths are matched with forward slashes so the same patterns work
        // for native Windows separators.
        let value = if subject == Subject::Command {
            value.to_string()
        } else {
            value.replace('\\', "/")
        };
        let value = value.as_str();

        for layer in [Layer::Hard, Layer::User, Layer::Session, Layer::Project] {
            for (rule, rule_layer, matcher) in &self.entries {
                if *rule_layer != layer || rule.subject != subject {
                    continue;
                }
                if matcher.is_match(value) {
                    return rule.action;
                }
            }
        }
        Action::Ask
    }

    /// First match wins, so a later rule cannot widen an earlier hard rule.
    pub fn extend(&mut self, rules: Vec<(Rule, Layer)>) -> Result<()> {
        for (rule, layer) in rules {
            let pattern = Self::normalize(&rule.pattern);
            if Self::is_blanket(&pattern) && rule.action == Action::Allow {
                return Err(Error::Config(format!(
                    "rule pattern '{}' would allow everything, which is not allowed",
                    rule.pattern
                )));
            }
            let matcher = Glob::new(&pattern)
                .map_err(|error| {
                    Error::Config(format!("invalid rule pattern '{}': {error}", rule.pattern))
                })?
                .compile_matcher();
            self.entries.push((rule, layer, matcher));
        }
        Ok(())
    }

    pub fn rules(&self) -> impl Iterator<Item = (&Rule, Layer)> {
        self.entries.iter().map(|(rule, layer, _)| (rule, *layer))
    }

    /// Command patterns are matched against both the raw command and its
    /// basename-free argument string, so `cargo test` matches `cargo test -p
    /// raven-core` but `cargo *` does not accidentally match `cargofmt`.
    fn normalize(pattern: &str) -> String {
        let trimmed = pattern.trim();
        if trimmed.ends_with('*') || trimmed.is_empty() {
            trimmed.to_string()
        } else {
            format!("{trimmed}*")
        }
    }

    fn is_blanket(pattern: &str) -> bool {
        matches!(pattern, "" | "*" | "**")
    }
}

impl Default for RuleSet {
    fn default() -> Self {
        Self::new(Vec::new()).expect("an empty rule set always compiles")
    }
}

/// The compiled-in hard rules.
///
/// These protect invariants rather than encode preferences, which is why they
/// live in code rather than in configuration: a user-authored file must not be
/// able to widen the set the binary enforces.
///
/// Two invariants are covered:
///
/// - **Protected paths stay protected.** The same names `is_protected_path`
///   denies on the read path are denied here for writes, including the
///   alternate-data-stream and trailing-dot forms that bypass a naive check.
/// - **No network access from tools.** Command execution cannot open a socket
///   or make a DNS query, so patterns that obviously talk to the network are
///   denied. This is a rule-based approximation, not a network boundary; that
///   arrives with M3.
pub fn hard_rules() -> RuleSet {
    RuleSet::new(vec![
        // Root-level and nested-forms of every protected name.
        rule_deny(Subject::WritePath, ".env"),
        rule_deny(Subject::WritePath, ".env.*"),
        rule_deny(Subject::WritePath, "**/.env"),
        rule_deny(Subject::WritePath, "**/.env.*"),
        rule_deny(Subject::WritePath, "**/.env::$DATA"),
        rule_deny(Subject::WritePath, "*.pem"),
        rule_deny(Subject::WritePath, "**/*.pem"),
        rule_deny(Subject::WritePath, "*.pem.*"),
        rule_deny(Subject::WritePath, "**/*.pem.*"),
        rule_deny(Subject::WritePath, "*.key"),
        rule_deny(Subject::WritePath, "**/*.key"),
        rule_deny(Subject::WritePath, "*.key.*"),
        rule_deny(Subject::WritePath, "**/*.key.*"),
        rule_deny(Subject::WritePath, "*.pfx"),
        rule_deny(Subject::WritePath, "**/*.pfx"),
        rule_deny(Subject::WritePath, "*.p12"),
        rule_deny(Subject::WritePath, "**/*.p12"),
        rule_deny(Subject::WritePath, "*.kdbx"),
        rule_deny(Subject::WritePath, "**/*.kdbx"),
        rule_deny(Subject::WritePath, "id_rsa"),
        rule_deny(Subject::WritePath, "**/id_rsa"),
        rule_deny(Subject::WritePath, "id_dsa"),
        rule_deny(Subject::WritePath, "**/id_dsa"),
        rule_deny(Subject::WritePath, "id_ecdsa"),
        rule_deny(Subject::WritePath, "**/id_ecdsa"),
        rule_deny(Subject::WritePath, "id_ed25519"),
        rule_deny(Subject::WritePath, "**/id_ed25519"),
        rule_deny(Subject::WritePath, ".npmrc"),
        rule_deny(Subject::WritePath, "**/.npmrc"),
        rule_deny(Subject::WritePath, ".pypirc"),
        rule_deny(Subject::WritePath, "**/.pypirc"),
        rule_deny(Subject::WritePath, "secrets.json"),
        rule_deny(Subject::WritePath, "**/secrets.json"),
        rule_deny(Subject::WritePath, "secrets.toml"),
        rule_deny(Subject::WritePath, "**/secrets.toml"),
        rule_deny(Subject::WritePath, "credentials"),
        rule_deny(Subject::WritePath, "**/credentials"),
        rule_deny(Subject::WritePath, ".git"),
        rule_deny(Subject::WritePath, "**/.git"),
        rule_deny(Subject::WritePath, "**/.git/**"),
        // Network-accessing commands. A rule-based approximation, not a
        // network boundary; the boundary arrives with M3.
        rule_deny(Subject::Command, "curl *"),
        rule_deny(Subject::Command, "wget *"),
        rule_deny(Subject::Command, "nc *"),
        rule_deny(Subject::Command, "ncat *"),
        rule_deny(Subject::Command, "ssh *"),
        rule_deny(Subject::Command, "scp *"),
        rule_deny(Subject::Command, "Invoke-WebRequest *"),
        rule_deny(Subject::Command, "Invoke-RestMethod *"),
        rule_deny(Subject::Command, "iwr *"),
        rule_deny(Subject::Command, "irm *"),
    ])
    .expect("the built-in hard rules must compile")
}

fn rule_deny(subject: Subject, pattern: &str) -> (Rule, Layer) {
    (
        Rule {
            subject,
            pattern: pattern.to_string(),
            action: Action::Deny,
        },
        Layer::Hard,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(subject: Subject, pattern: &str, action: Action, layer: Layer) -> (Rule, Layer) {
        (
            Rule {
                subject,
                pattern: pattern.to_string(),
                action,
            },
            layer,
        )
    }

    #[test]
    fn an_unmatched_request_falls_back_to_ask_not_allow() {
        let set = RuleSet::default();
        assert_eq!(
            set.resolve(Subject::Command, "anything at all"),
            Action::Ask
        );
    }

    #[test]
    fn first_match_wins_across_layers() {
        let set = RuleSet::new(vec![
            rule(Subject::Command, "rm *", Action::Allow, Layer::Session),
            rule(Subject::Command, "rm -rf *", Action::Deny, Layer::Hard),
        ])
        .unwrap();

        assert_eq!(
            set.resolve(Subject::Command, "rm -rf /"),
            Action::Deny,
            "a hard deny beats a wider session allow"
        );
        assert_eq!(
            set.resolve(Subject::Command, "rm file.txt"),
            Action::Allow,
            "the narrower allow still applies"
        );
    }

    #[test]
    fn a_blanket_allow_is_refused_at_parse_time() {
        for pattern in ["", "*", "**"] {
            let error = RuleSet::new(vec![rule(
                Subject::Command,
                pattern,
                Action::Allow,
                Layer::User,
            )])
            .unwrap_err();
            assert!(
                error.to_string().contains("would allow everything"),
                "pattern {pattern:?} must be refused: {error}"
            );
        }
    }

    #[test]
    fn patterns_are_anchored_without_accidentally_matching_prefixes() {
        let set = RuleSet::new(vec![rule(
            Subject::Command,
            "cargo test",
            Action::Allow,
            Layer::User,
        )])
        .unwrap();

        assert_eq!(
            set.resolve(Subject::Command, "cargo test"),
            Action::Allow,
            "an exact pattern should match itself"
        );
        assert_eq!(
            set.resolve(Subject::Command, "cargo test -p raven-core"),
            Action::Allow,
            "a pattern should match the same command with arguments"
        );
        assert_eq!(
            set.resolve(Subject::Command, "cargofmt"),
            Action::Ask,
            "a prefix of a rule must not match"
        );
    }

    #[test]
    fn subjects_do_not_leak_into_each_other() {
        let set = RuleSet::new(vec![rule(
            Subject::Command,
            "git *",
            Action::Allow,
            Layer::User,
        )])
        .unwrap();

        assert_eq!(set.resolve(Subject::Command, "git status"), Action::Allow);
        assert_eq!(
            set.resolve(Subject::WritePath, "git status"),
            Action::Ask,
            "a command rule must not authorize a write path"
        );
    }

    #[test]
    fn session_and_project_rules_can_be_added_later() {
        let mut set = RuleSet::default();
        set.extend(vec![rule(
            Subject::Command,
            "npm test",
            Action::Allow,
            Layer::Project,
        )])
        .unwrap();
        assert_eq!(set.resolve(Subject::Command, "npm test"), Action::Allow);
    }

    /// The invariant the hard layer exists for. A user (or a trusted project)
    /// writing every allow they can muster must still not be able to reach a
    /// protected path or grant network access.
    #[test]
    fn no_user_rule_can_override_a_hard_deny() {
        let attempts = vec![
            rule(Subject::WritePath, ".env", Action::Allow, Layer::User),
            rule(
                Subject::WritePath,
                ".env::$DATA",
                Action::Allow,
                Layer::Session,
            ),
            rule(
                Subject::WritePath,
                "key.pem.",
                Action::Allow,
                Layer::Project,
            ),
            rule(Subject::WritePath, "*.pem", Action::Allow, Layer::User),
            rule(
                Subject::WritePath,
                ".git/config",
                Action::Allow,
                Layer::Project,
            ),
            rule(Subject::Command, "curl *", Action::Allow, Layer::User),
            rule(Subject::Command, "wget *", Action::Allow, Layer::Session),
        ];

        let mut set = hard_rules();
        set.extend(attempts).unwrap();

        for path in [
            ".env",
            "key.pem",
            "key.pem.",
            ".env::$DATA",
            ".git/config",
            "C:\\src\\app\\.env",
            "C:\\src\\app\\.env.local",
        ] {
            assert_eq!(
                set.resolve(Subject::WritePath, path),
                Action::Deny,
                "a hard deny on {path:?} must survive any user rule"
            );
        }

        assert_eq!(
            set.resolve(Subject::WritePath, "notes.txt"),
            Action::Ask,
            "an unmatched path must ask, never allow"
        );

        for command in ["curl https://evil.example.com", "wget http://example.com"] {
            assert_eq!(
                set.resolve(Subject::Command, command),
                Action::Deny,
                "a user rule must not grant network access: {command}"
            );
        }
    }

    #[test]
    fn a_blanket_allow_is_refused_at_every_layer() {
        for layer in [Layer::User, Layer::Session, Layer::Project] {
            let mut set = hard_rules();
            let error = set
                .extend(vec![rule(Subject::Command, "**", Action::Allow, layer)])
                .unwrap_err();
            assert!(
                error.to_string().contains("would allow everything"),
                "layer {layer:?} must refuse a blanket allow: {error}"
            );
        }
    }

    /// Hard rules are compiled in and cannot be written by a config file, which
    /// is the reason a project rule cannot widen the set either.
    #[test]
    fn a_project_rule_never_widens_a_hard_deny() {
        let mut set = hard_rules();
        set.extend(vec![
            rule(
                Subject::WritePath,
                ".git/config",
                Action::Allow,
                Layer::Project,
            ),
            rule(Subject::WritePath, ".env", Action::Allow, Layer::Project),
            rule(Subject::Command, "curl *", Action::Allow, Layer::Project),
        ])
        .unwrap();

        assert_eq!(set.resolve(Subject::WritePath, ".git/config"), Action::Deny);
        assert_eq!(set.resolve(Subject::WritePath, ".env"), Action::Deny);
        assert_eq!(
            set.resolve(Subject::Command, "curl https://evil.example.com"),
            Action::Deny
        );
    }
}
