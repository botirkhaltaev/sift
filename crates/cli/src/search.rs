use std::path::Path;

use sift_core::Error as SiftError;
use sift_core::{
    CompiledSearch, Index, SearchFilter, SearchLineStyle, SearchMode, SearchRecordStyle,
    SearchStats,
};

use crate::cli::Cli;
use crate::filter::{build_search_filter_config, resolve_type_defs};
use crate::ignore::resolve_visibility_and_ignore;
use crate::output::{
    build_line_style_flags, effective_filename_mode, resolve_effective_line_number,
    resolve_glob_case_insensitive_from_args, resolve_json_from_args, resolve_line_number_from_args,
    resolve_null_from_args, search_output, write_search_stats,
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

pub fn run_files_mode(cli: &Cli) -> anyhow::Result<bool> {
    let args: Vec<String> = std::env::args().collect();
    let glob_case_insensitive = resolve_glob_case_insensitive_from_args(&args);
    let ignore_res = resolve_visibility_and_ignore(&args);
    let null_data = resolve_null_from_args(&args);

    let filter_ctx = crate::filter::SearchFilterCtx {
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

    let (filter_root, scopes, exclude_paths) = if let Ok(index) = Index::open(&cli.paths.sift_dir) {
        let prefixes = corpus_path_prefixes(&index.root, &cwd, &cli.search_scope.paths)?;
        let excludes = excluded_search_paths(&index.root, &cli.paths.sift_dir);
        (index.root, prefixes, excludes)
    } else {
        let root = cwd.canonicalize()?;
        let prefixes = walk_path_prefixes(&root, &cli.search_scope.paths)?;
        let excludes = excluded_search_paths(&root, &cli.paths.sift_dir);
        (root, prefixes, excludes)
    };

    let filter_config = build_search_filter_config(cli, filter_ctx, scopes, exclude_paths)?;
    let search_filter = SearchFilter::new(&filter_config, &filter_root)?;

    let paths = sift_core::walk_file_paths(&filter_root, search_filter.follow_links())?;
    let mut sorted_paths: Vec<_> = paths
        .into_iter()
        .filter(|p| search_filter.is_candidate(p))
        .collect();
    sorted_paths.sort();
    let sep = if null_data { '\0' } else { '\n' };
    let mut any = false;
    for p in &sorted_paths {
        any = true;
        let display = filter_root.join(p);
        print!("{}{sep}", display.display());
    }
    Ok(any)
}

impl Cli {
    pub fn run_search_with_index(
        &self,
        query: &CompiledSearch,
        index: &Index,
        cwd: &Path,
        out: &crate::output::SearchOutputCtx,
        filter: crate::filter::SearchFilterCtx,
    ) -> anyhow::Result<bool> {
        let prefixes = corpus_path_prefixes(&index.root, cwd, &self.search_scope.paths)?;
        let exclude_paths = excluded_search_paths(&index.root, &self.paths.sift_dir);
        let corpus_is_single_file = matches!(index.corpus_kind, sift_core::CorpusKind::File { .. });
        let filename_mode = effective_filename_mode(
            out.lines.with_filename,
            out.lines.is_path_mode,
            corpus_is_single_file,
        );
        let path_display = effective_path_display(&self.search_scope.paths);
        let line_number = resolve_effective_line_number(
            self.line_number_decl.line_number,
            out.lines.line_number,
            out.output_format,
        );
        let line_flags = build_line_style_flags(out, line_number);
        let output = search_output(
            out.output_format,
            out.mode.effective_mode,
            out.mode.quiet,
            SearchLineStyle {
                filename_mode,
                flags: line_flags,
                path_display,
                max_columns: out.max_columns,
                max_columns_preview: out.max_columns_preview,
            },
            SearchRecordStyle {
                null_data: out.format.null_data,
                color: out.format.color,
                path_separator: out.path_separator,
            },
            out.include_zero,
        );
        let filter_config = self.build_filter_config(filter, prefixes, exclude_paths)?;
        let search_filter = SearchFilter::new(&filter_config, &index.root)?;
        if out.print_stats {
            let mut stats = SearchStats::default();
            let ok = query.run_index_with_stats(
                index,
                &search_filter,
                output,
                &out.separators,
                &mut stats,
            )?;
            write_search_stats(&stats);
            return Ok(ok);
        }
        query
            .run_index(index, &search_filter, output, &out.separators)
            .map_err(Into::into)
    }

    pub fn run_search_walk(
        &self,
        query: &CompiledSearch,
        filter_root: &Path,
        out: &crate::output::SearchOutputCtx,
        filter: crate::filter::SearchFilterCtx,
    ) -> anyhow::Result<bool> {
        let prefixes = walk_path_prefixes(filter_root, &self.search_scope.paths)?;
        let exclude_paths = excluded_search_paths(filter_root, &self.paths.sift_dir);
        let filename_mode =
            effective_filename_mode(out.lines.with_filename, out.lines.is_path_mode, false);
        let path_display = effective_path_display(&self.search_scope.paths);
        let line_number = resolve_effective_line_number(
            self.line_number_decl.line_number,
            out.lines.line_number,
            out.output_format,
        );
        let line_flags = build_line_style_flags(out, line_number);
        let output = search_output(
            out.output_format,
            out.mode.effective_mode,
            out.mode.quiet,
            SearchLineStyle {
                filename_mode,
                flags: line_flags,
                path_display,
                max_columns: out.max_columns,
                max_columns_preview: out.max_columns_preview,
            },
            SearchRecordStyle {
                null_data: out.format.null_data,
                color: out.format.color,
                path_separator: out.path_separator,
            },
            out.include_zero,
        );
        let filter_config = self.build_filter_config(filter, prefixes, exclude_paths)?;
        let search_filter = SearchFilter::new(&filter_config, filter_root)?;
        if out.print_stats {
            let mut stats = SearchStats::default();
            let ok = query.run_walk_with_stats(
                filter_root,
                &search_filter,
                output,
                &out.separators,
                &mut stats,
            )?;
            write_search_stats(&stats);
            return Ok(ok);
        }
        query
            .run_walk(filter_root, &search_filter, output, &out.separators)
            .map_err(Into::into)
    }

    pub fn run_search(&self) -> anyhow::Result<bool> {
        let patterns = resolve_patterns(
            &self.patterns.regexp,
            self.patterns.pattern_file.as_deref(),
            self.patterns.pattern.as_deref(),
        )?;

        let args: Vec<String> = std::env::args().collect();
        let invert_match = resolve_invert_match_from_args(&args);
        let (mode, only_matching, quiet) = resolve_output_mode(&args, invert_match);
        let use_json = resolve_json_from_args(&args);

        let pretty = self.column_decl.pretty;
        let vimgrep = self.column_decl.vimgrep;

        let line_number_override = if pretty || vimgrep {
            Some(true)
        } else {
            resolve_line_number_from_args(&args)
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

        let opts = self.build_search_opts(&args, only_matching);
        let query = CompiledSearch::new(&patterns, opts).map_err(|e| anyhow::anyhow!("{e}"))?;
        let cwd = std::env::current_dir()?;

        let (out, filter) =
            self.build_output_and_filter(&args, effective_mode, quiet, line_number_override);

        match Index::open(&self.paths.sift_dir) {
            Ok(index) => self.run_search_with_index(&query, &index, &cwd, &out, filter),
            Err(
                SiftError::MissingMeta(_)
                | SiftError::MissingComponent(_)
                | SiftError::InvalidMeta(_),
            ) => {
                let filter_root = cwd.canonicalize().map_err(|e| anyhow::anyhow!("{e}"))?;
                self.run_search_walk(&query, &filter_root, &out, filter)
            }
            Err(e) => Err(anyhow::anyhow!("{e}")),
        }
    }
}
