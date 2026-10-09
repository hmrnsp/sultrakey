//! Errors meant for the infra team: each problem is one plain sentence, usually followed
//! by the exact command that fixes it. The exit code tells pm2/systemd whether a restart
//! can help (78 = configuration problem, restarting is pointless).

use std::fmt;

/// Exit code families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Wrong use of the command line (EX_USAGE).
    Usage,
    /// Configuration problem: empty key, cannot decrypt, permissions, wrong key (EX_CONFIG).
    Config,
    /// Anything else.
    Other,
}

impl Code {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Usage => 64,
            Self::Config => 78,
            Self::Other => 1,
        }
    }
}

/// One problem and, when known, the command that solves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub problem: String,
    pub solution: Option<String>,
}

impl Issue {
    pub fn new(problem: impl Into<String>, solution: impl Into<String>) -> Self {
        Self {
            problem: problem.into(),
            solution: Some(solution.into()),
        }
    }

    pub fn bare(problem: impl Into<String>) -> Self {
        Self {
            problem: problem.into(),
            solution: None,
        }
    }
}

/// A failure with one or more issues (`check` reports everything it finds at once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fail {
    pub code: Code,
    pub issues: Vec<Issue>,
}

impl Fail {
    pub fn new(code: Code, issue: Issue) -> Self {
        Self {
            code,
            issues: vec![issue],
        }
    }

    pub fn usage(problem: impl Into<String>, solution: impl Into<String>) -> Self {
        Self::new(Code::Usage, Issue::new(problem, solution))
    }

    pub fn config(problem: impl Into<String>, solution: impl Into<String>) -> Self {
        Self::new(Code::Config, Issue::new(problem, solution))
    }

    pub fn config_bare(problem: impl Into<String>) -> Self {
        Self::new(Code::Config, Issue::bare(problem))
    }

    pub fn other(problem: impl Into<String>) -> Self {
        Self::new(Code::Other, Issue::bare(problem))
    }

    pub fn exit_code(&self) -> i32 {
        self.code.exit_code()
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let problems: Vec<&str> = self.issues.iter().map(|i| i.problem.as_str()).collect();
        write!(f, "{}", problems.join("; "))
    }
}

impl std::error::Error for Fail {}

/// The user stopped a prompt. Not a failure of the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abort {
    /// "Tidak" to a confirmation.
    Cancelled,
}

impl Abort {
    pub fn exit_code(self) -> i32 {
        1
    }
}

impl fmt::Display for Abort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Dibatalkan; tidak ada yang diubah."),
        }
    }
}

impl std::error::Error for Abort {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(Fail::usage("a", "b").exit_code(), 64);
        assert_eq!(Fail::config("a", "b").exit_code(), 78);
        assert_eq!(Fail::other("a").exit_code(), 1);
        assert_eq!(Abort::Cancelled.exit_code(), 1);
    }

    #[test]
    fn display_joins_problems() {
        let fail = Fail {
            code: Code::Config,
            issues: vec![Issue::bare("A kosong."), Issue::new("B rusak.", "x")],
        };
        assert_eq!(fail.to_string(), "A kosong.; B rusak.");
    }
}
