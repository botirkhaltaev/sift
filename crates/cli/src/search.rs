use std::path::PathBuf;

use sift_core::{
    CandidateFilter, ColumnLimit, ColumnOverflow, CorpusKind, Indexes, RecordTerminator,
    SearchLineStyle, SearchMode, SearchQuery, SearchRecordStyle,
};

use crate::cli::Cli;
use crate::filter::{SearchFilterCtx, build_search_filter_config, resolve_type_defs};
use crate::ignore::resolve_visibility_and_ignore;
use crate::output::{
    FilenameContext, SearchOutputCtx, build_line_style_flags, effective_filename_mode,
    resolve_effective_line_number, resolve_glob_case_insensitive_from_args, resolve_json_from_args,
    resolve_line_number_from_args, resolve_null_from_args, search_output, write_search_stats,
};
use crate::paths::{
    corpus_path_prefixes, effective_path_display, excluded_search_paths, walk_path_prefixes,
};
use crate::pattern::{resolve_invert_match_from_args, resolve_output_mode, resolve_patterns};

pub fn run_type_list(cli: &Cli) {
    let defs = resolve_type_defs(&cli.filter_decl);
    let mut sorted = defs;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for def in &sorted {
        println!("{}: {}", def.name, def.globs.join(", "));
    }
}

/// # Errors
///
/// Returns an error if I/O operations fail, paths are invalid, or filter config building fails.
pub fn run_files_mode(cli: &Cli, args: &[String]) -> anyhow::Result<bool> {
    let glob_case_insensitive = resolve_glob_case_insensitive_from_args(args);
    let ignore_res = resolve_visibility_and_ignore(args);
    let null_data = resolve_null_from_args(args);

    let filter_ctx = SearchFilterCtx {
        hidden: ignore_res.hidden,
        ignore_sources: ignore_res.sources,
        require_git: ignore_res.require_git,
        glob_case_insensitive,
        msg_flags: ignore_res.msg_flags,
    };

    if let Some(threads) = cli.threading.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .ok();
    }

    let cwd = std::env::current_dir()?;

    let indexes = Indexes::open(&cli.paths.sift_dir)?;

    let (filter_root, scopes, exclude_paths) = if indexes.is_empty() {
        let root = cwd.canonicalize()?;
        let prefixes = walk_path_prefixes(&root, &cli.search_scope.paths)?;
        let excludes = excluded_search_paths(&root, &cli.paths.sift_dir);
        (root, prefixes, excludes)
    } else {
        let prefixes = corpus_path_prefixes(indexes.root(), &cwd, &cli.search_scope.paths)?;
        let excludes = excluded_search_paths(indexes.root(), &cli.paths.sift_dir);
        (indexes.root().to_path_buf(), prefixes, excludes)
    };

    let filter_config = build_search_filter_config(cli, filter_ctx, scopes.clone(), exclude_paths)?;
    let search_filter = CandidateFilter::new(&filter_config, &filter_root)?;

    let walk_opts = sift_core::WalkOptions {
        links: if search_filter.follow_links() {
            sift_core::LinkTraversal::Follow
        } else {
            sift_core::LinkTraversal::DoNotFollow
        },
        max_depth: search_filter.max_depth(),
        max_filesize: search_filter.max_filesize(),
        one_file_system: search_filter.one_file_system(),
    };

    let effective_scopes = if scopes.is_empty() {
        vec![PathBuf::from("")]
    } else {
        scopes
    };

    let mut all_paths: Vec<_> = Vec::new();
    for scope in &effective_scopes {
        let scope_path = if scope.as_os_str().is_empty() {
            filter_root.clone()
        } else {
            filter_root.join(scope)
        };
        if !scope_path.exists() {
            continue;
        }
        let scope_path = scope_path.canonicalize().unwrap_or(scope_path);
        if scope_path.is_file() {
            let rel = scope_path
                .strip_prefix(&filter_root)
                .unwrap_or(&scope_path)
                .to_path_buf();
            if search_filter.matches_path(&rel) {
                all_paths.push(rel);
            }
        } else if scope_path.is_dir() {
            let discovered = sift_core::discover_files(&scope_path, walk_opts)?;
            for rel_in_scope in discovered {
                let full_rel = if scope.as_os_str().is_empty() {
                    rel_in_scope
                } else {
                    scope.join(rel_in_scope)
                };
                if search_filter.matches_path(&full_rel) {
                    all_paths.push(full_rel);
                }
            }
        }
    }
    all_paths.sort();
    all_paths.dedup();
    let sep = if null_data { '\0' } else { '\n' };
    let mut any = false;
    for p in &all_paths {
        any = true;
        let display = filter_root.join(p);
        print!("{}{sep}", display.display());
    }
    Ok(any)
}

struct SearchCtx {
    filter_root: PathBuf,
    prefixes: Vec<PathBuf>,
    exclude_paths: Vec<PathBuf>,
}

impl Cli {
    /// # Errors
    ///
    /// Returns an error if no pattern is given, pattern file I/O fails, or search execution fails.
    pub fn run_search(&self, args: &[String]) -> anyhow::Result<bool> {
        let patterns = resolve_patterns(
            &self.patterns.regexp,
            self.patterns.pattern_file.as_deref(),
            self.patterns.pattern.as_deref(),
        )?;

        let invert_match = resolve_invert_match_from_args(args);
        let (mode, only_matching, quiet) = resolve_output_mode(args, invert_match);
        let use_json = resolve_json_from_args(args);

        let pretty = self.column_decl.pretty;
        let vimgrep = self.column_decl.vimgrep;

        let line_number_override = if pretty || vimgrep {
            Some(true)
        } else {
            resolve_line_number_from_args(args)
        };

        let effective_mode = if only_matching {
            SearchMode::OnlyMatching
        } else {
            mode
        };

        if use_json {
            match effective_mode {
                SearchMode::Count
                | SearchMode::CountMatches
                | SearchMode::FilesWithMatches
                | SearchMode::FilesWithoutMatch => {
                    anyhow::bail!(
                        "sift: --json cannot be used with --count, --count-matches, --files-with-matches, or --files-without-match"
                    );
                }
                SearchMode::Standard | SearchMode::OnlyMatching => {}
            }
        }

        if let Some(threads) = self.threading.threads {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build_global()
                .ok();
        }

        let opts = self.build_search_opts(args, only_matching);
        let query = SearchQuery::new(&patterns, opts).map_err(|e| anyhow::anyhow!("{e}"))?;
        let cwd = std::env::current_dir()?;

        let (out, filter) =
            self.build_output_and_filter(args, effective_mode, quiet, line_number_override);

        let indexes = Indexes::open(&self.paths.sift_dir)?;

        let ctx = if indexes.is_empty() {
            let root = cwd.canonicalize().map_err(|e| anyhow::anyhow!("{e}"))?;
            let prefixes = walk_path_prefixes(&root, &self.search_scope.paths)?;
            let exclude_paths = excluded_search_paths(&root, &self.paths.sift_dir);

            let sift_dir = self.paths.sift_dir.clone();
            let init_root = root.clone();
            std::thread::spawn(move || {
                crate::spawn_daemon(&sift_dir, Some(&init_root));
            });

            SearchCtx {
                filter_root: root,
                prefixes,
                exclude_paths,
            }
        } else {
            let prefixes = corpus_path_prefixes(indexes.root(), &cwd, &self.search_scope.paths)?;
            let exclude_paths = excluded_search_paths(indexes.root(), &self.paths.sift_dir);
            SearchCtx {
                filter_root: indexes.root().to_path_buf(),
                prefixes,
                exclude_paths,
            }
        };

        let filename_ctx = if out.lines.is_path_mode {
            FilenameContext::PathMode
        } else {
            match indexes.corpus_kind() {
                Some(CorpusKind::SingleFile) => FilenameContext::SingleFileCorpus,
                _ => FilenameContext::DirectoryCorpus,
            }
        };
        let output = self.build_search_output(&out, filename_ctx);

        self.execute_search(&query, &indexes, &ctx, &output, &out, filter)
    }

    fn build_search_output(
        &self,
        out: &SearchOutputCtx,
        filename_ctx: FilenameContext,
    ) -> sift_core::SearchOutput {
        let path_display = effective_path_display(&self.search_scope.paths);
        let line_number = resolve_effective_line_number(
            self.line_number_decl.line_number,
            out.lines.line_number,
            out.output_format,
        );
        let line_flags = build_line_style_flags(out, line_number);
        search_output(
            out.output_format,
            out.mode.effective_mode,
            out.mode.quiet,
            SearchLineStyle {
                filename_mode: effective_filename_mode(out.lines.with_filename, filename_ctx),
                flags: line_flags,
                path_display,
                columns: out.max_columns.map(|max| ColumnLimit {
                    max,
                    overflow: if out.max_columns_preview {
                        ColumnOverflow::Preview
                    } else {
                        ColumnOverflow::Omit
                    },
                }),
            },
            SearchRecordStyle {
                terminator: if out.format.null_data {
                    RecordTerminator::Nul
                } else {
                    RecordTerminator::Newline
                },
                color: out.format.color,
                path_separator: out.path_separator,
            },
            out.include_zero,
        )
    }

    fn execute_search(
        &self,
        query: &SearchQuery,
        indexes: &Indexes,
        ctx: &SearchCtx,
        output: &sift_core::SearchOutput,
        out: &SearchOutputCtx,
        filter: SearchFilterCtx,
    ) -> anyhow::Result<bool> {
        let filter_config =
            self.build_filter_config(filter, ctx.prefixes.clone(), ctx.exclude_paths.clone())?;
        let search_filter = CandidateFilter::new(&filter_config, &ctx.filter_root)?;
        let outcome = sift_core::grep::run(
            query,
            &sift_core::grep::GrepRequest {
                indexes,
                filter: &search_filter,
                output: *output,
                separators: &out.separators,
                collect_stats: out.print_stats,
            },
        )?;
        if let Some(s) = &outcome.stats {
            write_search_stats(s);
        }
        Ok(outcome.matched)
    }
}
