use clap::{ArgAction, Args};
use sift_core::IgnoreSources;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct MessageFlags: u8 {
        const NO_MESSAGES        = 1 << 0;
        const NO_IGNORE_MESSAGES = 1 << 1;
        const NO_IGNORE_FILES    = 1 << 2;
    }
}

/// Resolved visibility / ignore state from argv.
pub struct IgnoreResolution {
    pub hidden: bool,
    pub sources: IgnoreSources,
    pub require_git: bool,
    pub msg_flags: MessageFlags,
}

/// Hidden files, ignore rules, and `require_git` — processed in argv order (ripgrep-style).
pub fn resolve_visibility_and_ignore(args: &[String]) -> IgnoreResolution {
    const DEFAULT_SOURCES: IgnoreSources = IgnoreSources::DOT
        .union(IgnoreSources::VCS)
        .union(IgnoreSources::EXCLUDE)
        .union(IgnoreSources::GLOBAL)
        .union(IgnoreSources::PARENT);

    let mut sources = DEFAULT_SOURCES;
    let mut require_git = true;
    let mut hidden = false;
    let mut u_count: u8 = 0;
    let mut msg_flags = MessageFlags::empty();

    for arg in args {
        if arg == "--unrestricted" {
            u_count = u_count.saturating_add(1).min(3);
            if u_count == 1 {
                sources = IgnoreSources::empty();
            } else if u_count == 2 {
                hidden = true;
            }
            continue;
        }
        if arg.len() >= 2 {
            let bytes = arg.as_bytes();
            if bytes[0] == b'-' && bytes.get(1) != Some(&b'-') && arg[1..].chars().all(|c| c == 'u')
            {
                for _ in 0..arg.len().saturating_sub(1) {
                    u_count = u_count.saturating_add(1).min(3);
                    if u_count == 1 {
                        sources = IgnoreSources::empty();
                    } else if u_count == 2 {
                        hidden = true;
                    }
                }
                continue;
            }
        }

        match arg.as_str() {
            "--no-ignore" => sources = IgnoreSources::empty(),
            "--ignore" => sources = DEFAULT_SOURCES,
            "--no-ignore-vcs" => sources.remove(IgnoreSources::VCS),
            "--ignore-vcs" => sources.insert(IgnoreSources::VCS),
            "--no-ignore-dot" => sources.remove(IgnoreSources::DOT),
            "--ignore-dot" => sources.insert(IgnoreSources::DOT),
            "--no-ignore-global" => sources.remove(IgnoreSources::GLOBAL),
            "--ignore-global" => sources.insert(IgnoreSources::GLOBAL),
            "--no-ignore-exclude" => sources.remove(IgnoreSources::EXCLUDE),
            "--ignore-exclude" => sources.insert(IgnoreSources::EXCLUDE),
            "--no-ignore-parent" => sources.remove(IgnoreSources::PARENT),
            "--ignore-parent" => sources.insert(IgnoreSources::PARENT),
            "--no-require-git" => require_git = false,
            "--require-git" => require_git = true,
            "--hidden" | "-." => hidden = true,
            "--no-hidden" => hidden = false,
            "--no-messages" => msg_flags.insert(MessageFlags::NO_MESSAGES),
            "--messages" => msg_flags.remove(MessageFlags::NO_MESSAGES),
            "--no-ignore-messages" => msg_flags.insert(MessageFlags::NO_IGNORE_MESSAGES),
            "--ignore-messages" => msg_flags.remove(MessageFlags::NO_IGNORE_MESSAGES),
            "--no-ignore-files" => msg_flags.insert(MessageFlags::NO_IGNORE_FILES),
            "--ignore-files" => msg_flags.remove(MessageFlags::NO_IGNORE_FILES),
            _ => {}
        }
    }

    IgnoreResolution {
        hidden,
        sources,
        require_git,
        msg_flags,
    }
}

// ── Clap declarations (ignore/hidden/messages flags) ──

/// Clap declarations only; effective values come from [`resolve_visibility_and_ignore`].
#[derive(Args)]
pub struct IgnoreNoDecl {
    #[arg(long = "no-ignore", action = ArgAction::SetTrue)]
    pub _no_ignore: bool,
    #[arg(long = "ignore", action = ArgAction::SetTrue)]
    pub _ignore: bool,
}

#[derive(Args)]
pub struct IgnoreVcsDecl {
    #[arg(long = "no-ignore-vcs", action = ArgAction::SetTrue)]
    pub _no_ignore_vcs: bool,
    #[arg(long = "ignore-vcs", action = ArgAction::SetTrue)]
    pub _ignore_vcs: bool,
}

#[derive(Args)]
pub struct IgnoreDotDecl {
    #[arg(long = "no-ignore-dot", action = ArgAction::SetTrue)]
    pub _no_ignore_dot: bool,
    #[arg(long = "ignore-dot", action = ArgAction::SetTrue)]
    pub _ignore_dot: bool,
}

#[derive(Args)]
pub struct IgnoreGitDecl {
    #[arg(long = "no-require-git", action = ArgAction::SetTrue)]
    pub _no_require_git: bool,
    #[arg(long = "require-git", action = ArgAction::SetTrue)]
    pub _require_git: bool,
}

#[derive(Args)]
pub struct IgnoreGlobalDecl {
    #[arg(long = "no-ignore-global", action = ArgAction::SetTrue)]
    pub _no_ignore_global: bool,
    #[arg(long = "ignore-global", action = ArgAction::SetTrue)]
    pub _ignore_global: bool,
}

#[derive(Args)]
pub struct IgnoreExcludeDecl {
    #[arg(long = "no-ignore-exclude", action = ArgAction::SetTrue)]
    pub _no_ignore_exclude: bool,
    #[arg(long = "ignore-exclude", action = ArgAction::SetTrue)]
    pub _ignore_exclude: bool,
}

#[derive(Args)]
pub struct IgnoreParentDecl {
    #[arg(long = "no-ignore-parent", action = ArgAction::SetTrue)]
    pub _no_ignore_parent: bool,
    #[arg(long = "ignore-parent", action = ArgAction::SetTrue)]
    pub _ignore_parent: bool,
}

#[derive(Args)]
pub struct IgnoreFilesDecl {
    #[arg(long = "no-ignore-files", action = ArgAction::SetTrue)]
    pub _no_ignore_files: bool,
    #[arg(long = "ignore-files", action = ArgAction::SetTrue)]
    pub _ignore_files: bool,
}

#[derive(Args)]
pub struct MessagesDecl {
    #[arg(long = "no-messages", action = ArgAction::SetTrue)]
    pub _no_messages: bool,
    #[arg(long = "messages", action = ArgAction::SetTrue)]
    pub _messages: bool,
}

#[derive(Args)]
pub struct IgnoreMessagesDecl {
    #[arg(long = "no-ignore-messages", action = ArgAction::SetTrue)]
    pub _no_ignore_messages: bool,
    #[arg(long = "ignore-messages", action = ArgAction::SetTrue)]
    pub _ignore_messages: bool,
}

#[derive(Args)]
pub struct UnrestrictedDecl {
    #[arg(short = 'u', long = "unrestricted", action = ArgAction::Count)]
    pub _unrestricted: u8,
}

/// Declares `-A`/`-B`/`-C` for clap; effective values use [`resolve_context_from_args`].
#[derive(Args)]
pub struct ContextDecl {
    #[arg(short = 'A', long = "after-context", value_name = "NUM", action = ArgAction::Append)]
    pub _after: Vec<usize>,
    #[arg(short = 'B', long = "before-context", value_name = "NUM", action = ArgAction::Append)]
    pub _before: Vec<usize>,
    #[arg(short = 'C', long = "context", value_name = "NUM", action = ArgAction::Append)]
    pub _context: Vec<usize>,
}
