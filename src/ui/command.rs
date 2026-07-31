//! The command table.
//!
//! One definition drives the `:` completion menu, the help popup and the
//! documentation, so a new command cannot appear in one place and be missing
//! from the others.

/// What a command's argument is, which decides how it is completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// Takes no argument.
    None,
    /// A directory path.
    Directory,
    /// The name of an installed theme.
    Theme,
    /// Free text (a new file name, a shell command): nothing to complete.
    Text,
}

/// One command, including its aliases.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// Names that invoke it; the first is the canonical one.
    pub names: &'static [&'static str],
    /// Argument placeholder shown in the menu, empty when there is none.
    pub args: &'static str,
    /// One-line description.
    pub description: &'static str,
    /// How to complete the argument.
    pub arg_kind: ArgKind,
}

impl CommandSpec {
    /// `:q / :quit`, as shown in the help popup.
    pub fn display_names(&self) -> String {
        self.names
            .iter()
            .map(|n| format!(":{n}"))
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

/// Every command the palette accepts.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        names: &["q", "quit"],
        args: "",
        description: "Quit",
        arg_kind: ArgKind::None,
    },
    CommandSpec {
        names: &["w", "write"],
        args: "",
        description: "Save the configuration",
        arg_kind: ArgKind::None,
    },
    CommandSpec {
        names: &["so", "source"],
        args: "",
        description: "Reload the configuration",
        arg_kind: ArgKind::None,
    },
    CommandSpec {
        names: &["e", "cd"],
        args: "<path>",
        description: "Navigate to a directory",
        arg_kind: ArgKind::Directory,
    },
    CommandSpec {
        names: &["mkdir"],
        args: "<name>",
        description: "Create a directory",
        arg_kind: ArgKind::Text,
    },
    CommandSpec {
        names: &["touch"],
        args: "<name>",
        description: "Create an empty file",
        arg_kind: ArgKind::Text,
    },
    CommandSpec {
        names: &["rename"],
        args: "<new>",
        description: "Rename the current entry",
        arg_kind: ArgKind::Text,
    },
    CommandSpec {
        names: &["delete"],
        args: "",
        description: "Trash the selected or current entries",
        arg_kind: ArgKind::None,
    },
    CommandSpec {
        names: &["theme"],
        args: "[name]",
        description: "Switch theme, or list the available ones",
        arg_kind: ArgKind::Theme,
    },
    CommandSpec {
        names: &["trash"],
        args: "",
        description: "Browse the trash",
        arg_kind: ArgKind::None,
    },
    CommandSpec {
        names: &["shell"],
        args: "",
        description: "Open an interactive subshell",
        arg_kind: ArgKind::None,
    },
    CommandSpec {
        names: &["!"],
        args: "<cmd>",
        description: "Run a shell command (%f = selected paths)",
        arg_kind: ArgKind::Text,
    },
    CommandSpec {
        names: &["help"],
        args: "",
        description: "Show the help popup",
        arg_kind: ArgKind::None,
    },
];

/// Looks a command up by any of its names.
pub fn find(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.names.contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let mut seen = Vec::new();
        for spec in COMMANDS {
            for name in spec.names {
                assert!(!seen.contains(name), "duplicate command name: {name}");
                seen.push(name);
            }
        }
    }

    #[test]
    fn every_command_is_described() {
        for spec in COMMANDS {
            assert!(!spec.names.is_empty());
            assert!(!spec.description.is_empty(), "{:?}", spec.names);
        }
    }

    #[test]
    fn commands_with_arguments_declare_a_kind() {
        for spec in COMMANDS {
            assert_eq!(
                spec.args.is_empty(),
                spec.arg_kind == ArgKind::None,
                "{:?} disagrees about taking an argument",
                spec.names
            );
        }
    }

    #[test]
    fn lookup_finds_aliases() {
        assert_eq!(find("quit").unwrap().names[0], "q");
        assert_eq!(find("cd").unwrap().names[0], "e");
        assert!(find("nonexistent").is_none());
    }

    #[test]
    fn display_names_join_aliases() {
        assert_eq!(find("q").unwrap().display_names(), ":q / :quit");
        assert_eq!(find("trash").unwrap().display_names(), ":trash");
    }
}
