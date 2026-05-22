//! Named search scenarios for `sift-profile` (aligned with Criterion `search` bench).

use std::path::PathBuf;

use sift_core::{
    CaseMode, ColorChoice, FilenameMode, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources,
    LineStyleFlags, PathDisplay, SearchFilterConfig, SearchLineStyle, SearchMatchFlags, SearchMode,
    SearchOptions, SearchOutput, SearchRecordStyle, VisibilityConfig,
};

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub patterns: Vec<String>,
    pub opts: SearchOptions,
    pub filter_config: SearchFilterConfig,
    pub output: SearchOutput,
}

impl Scenario {
    pub const fn new(
        name: &'static str,
        patterns: Vec<String>,
        opts: SearchOptions,
        filter_config: SearchFilterConfig,
        output: SearchOutput,
    ) -> Self {
        Self {
            name,
            patterns,
            opts,
            filter_config,
            output,
        }
    }

    pub fn default_filter() -> SearchFilterConfig {
        SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Respect,
                ignore: IgnoreConfig {
                    sources: IgnoreSources::DOT | IgnoreSources::VCS | IgnoreSources::EXCLUDE,
                    custom_files: Vec::new(),
                    require_git: true,
                },
            },
            ..SearchFilterConfig::default()
        }
    }
}

const fn make_output(mode: SearchMode, emission: sift_core::OutputEmission) -> SearchOutput {
    SearchOutput {
        format: sift_core::SearchOutputFormat::Text,
        mode,
        emission,
        lines: SearchLineStyle {
            filename_mode: FilenameMode::Auto,
            flags: LineStyleFlags::empty(),
            path_display: PathDisplay::Relative,
            max_columns: None,
            max_columns_preview: false,
        },
        records: SearchRecordStyle {
            null_data: false,
            color: ColorChoice::Never,
            path_separator: None,
        },
        passthru: false,
        include_zero: false,
    }
}

const fn default_output() -> SearchOutput {
    make_output(SearchMode::Standard, sift_core::OutputEmission::Quiet)
}

fn literal_narrow() -> Scenario {
    Scenario::new(
        "literal_narrow",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        default_output(),
    )
}

fn word_literal() -> Scenario {
    Scenario::new(
        "word_literal",
        vec!["beta".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::WORD_REGEXP,
            case_mode: CaseMode::Sensitive,
            max_results: None,
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        default_output(),
    )
}

fn line_literal() -> Scenario {
    Scenario::new(
        "line_literal",
        vec!["beta".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::LINE_REGEXP,
            case_mode: CaseMode::Sensitive,
            max_results: None,
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        default_output(),
    )
}

fn fixed_string() -> Scenario {
    Scenario::new(
        "fixed_string",
        vec!["beta.gamma".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::FIXED_STRINGS,
            case_mode: CaseMode::Sensitive,
            max_results: None,
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        default_output(),
    )
}

fn casei_literal() -> Scenario {
    Scenario::new(
        "casei_literal",
        vec!["beta".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::default(),
            case_mode: CaseMode::Insensitive,
            max_results: None,
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        default_output(),
    )
}

fn smart_case_lower() -> Scenario {
    Scenario::new(
        "smart_case_lower",
        vec!["beta".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::default(),
            case_mode: CaseMode::Smart,
            max_results: None,
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        default_output(),
    )
}

fn smart_case_upper() -> Scenario {
    Scenario::new(
        "smart_case_upper",
        vec!["Beta".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::default(),
            case_mode: CaseMode::Smart,
            max_results: None,
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        default_output(),
    )
}

fn required_literal() -> Scenario {
    Scenario::new(
        "required_literal",
        vec!["[A-Z]+_RESUME".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        default_output(),
    )
}

fn no_literal() -> Scenario {
    Scenario::new(
        "no_literal",
        vec![r"\w{5}\s+\w{5}\s+\w{5}\s+\w{5}\s+\w{5}".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        default_output(),
    )
}

fn alternation() -> Scenario {
    Scenario::new(
        "alternation",
        vec!["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        default_output(),
    )
}

fn alternation_casei() -> Scenario {
    Scenario::new(
        "alternation_casei",
        vec!["ERR_SYS|PME_TURN_OFF|LINK_REQ_RST|CFG_BME_EVT".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::default(),
            case_mode: CaseMode::Insensitive,
            max_results: None,
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        default_output(),
    )
}

fn unicode_class() -> Scenario {
    Scenario::new(
        "unicode_class",
        vec![r"\p{Greek}".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        default_output(),
    )
}

fn glob_include() -> Scenario {
    Scenario::new(
        "glob_include",
        vec!["beta".to_string()],
        SearchOptions::default(),
        SearchFilterConfig {
            glob: GlobConfig {
                patterns: vec!["**/*.txt".to_string()],
                case_insensitive: false,
            },
            ..Scenario::default_filter()
        },
        default_output(),
    )
}

fn glob_exclude() -> Scenario {
    Scenario::new(
        "glob_exclude",
        vec!["beta".to_string()],
        SearchOptions::default(),
        SearchFilterConfig {
            glob: GlobConfig {
                patterns: vec!["!**/*.txt".to_string()],
                case_insensitive: false,
            },
            ..Scenario::default_filter()
        },
        default_output(),
    )
}

fn glob_casei() -> Scenario {
    Scenario::new(
        "glob_casei",
        vec!["beta".to_string()],
        SearchOptions::default(),
        SearchFilterConfig {
            glob: GlobConfig {
                patterns: vec!["**/*.TXT".to_string()],
                case_insensitive: true,
            },
            ..Scenario::default_filter()
        },
        default_output(),
    )
}

fn hidden_default() -> Scenario {
    Scenario::new(
        "hidden_default",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        default_output(),
    )
}

fn hidden_include() -> Scenario {
    Scenario::new(
        "hidden_include",
        vec!["beta".to_string()],
        SearchOptions::default(),
        SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Include,
                ..Scenario::default_filter().visibility
            },
            ..Scenario::default_filter()
        },
        default_output(),
    )
}

fn ignore_default() -> Scenario {
    Scenario::new(
        "ignore_default",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        default_output(),
    )
}

fn ignore_custom() -> Scenario {
    Scenario::new(
        "ignore_custom",
        vec!["beta".to_string()],
        SearchOptions::default(),
        SearchFilterConfig {
            visibility: VisibilityConfig {
                hidden: HiddenMode::Respect,
                ignore: IgnoreConfig {
                    sources: IgnoreSources::empty(),
                    custom_files: vec![PathBuf::from(".ignore")],
                    require_git: false,
                },
            },
            ..SearchFilterConfig::default()
        },
        default_output(),
    )
}

fn scoped_search() -> Scenario {
    Scenario::new(
        "scoped_search",
        vec!["beta".to_string()],
        SearchOptions::default(),
        SearchFilterConfig {
            scopes: vec![PathBuf::from("subdir")],
            ..Scenario::default_filter()
        },
        make_output(
            SearchMode::FilesWithMatches,
            sift_core::OutputEmission::Normal,
        ),
    )
}

fn only_matching() -> Scenario {
    Scenario::new(
        "only_matching",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        make_output(SearchMode::OnlyMatching, sift_core::OutputEmission::Normal),
    )
}

fn count() -> Scenario {
    Scenario::new(
        "count",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        make_output(SearchMode::Count, sift_core::OutputEmission::Normal),
    )
}

fn count_matches() -> Scenario {
    Scenario::new(
        "count_matches",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        make_output(SearchMode::CountMatches, sift_core::OutputEmission::Normal),
    )
}

fn files_with_matches() -> Scenario {
    Scenario::new(
        "files_with_matches",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        make_output(
            SearchMode::FilesWithMatches,
            sift_core::OutputEmission::Normal,
        ),
    )
}

fn files_without_match() -> Scenario {
    Scenario::new(
        "files_without_match",
        vec!["beta".to_string()],
        SearchOptions::default(),
        Scenario::default_filter(),
        make_output(
            SearchMode::FilesWithoutMatch,
            sift_core::OutputEmission::Normal,
        ),
    )
}

fn max_count_1() -> Scenario {
    Scenario::new(
        "max_count_1",
        vec!["beta".to_string()],
        SearchOptions {
            flags: SearchMatchFlags::default(),
            case_mode: CaseMode::Sensitive,
            max_results: Some(1),
            ..SearchOptions::default()
        },
        Scenario::default_filter(),
        make_output(SearchMode::Standard, sift_core::OutputEmission::Normal),
    )
}

#[allow(clippy::type_complexity)]
const ALL_SCENARIOS: &[(&str, fn() -> Scenario)] = &[
    ("literal_narrow", literal_narrow),
    ("word_literal", word_literal),
    ("line_literal", line_literal),
    ("fixed_string", fixed_string),
    ("casei_literal", casei_literal),
    ("smart_case_lower", smart_case_lower),
    ("smart_case_upper", smart_case_upper),
    ("required_literal", required_literal),
    ("no_literal", no_literal),
    ("alternation", alternation),
    ("alternation_casei", alternation_casei),
    ("unicode_class", unicode_class),
    ("glob_include", glob_include),
    ("glob_exclude", glob_exclude),
    ("glob_casei", glob_casei),
    ("hidden_default", hidden_default),
    ("hidden_include", hidden_include),
    ("ignore_default", ignore_default),
    ("ignore_custom", ignore_custom),
    ("scoped_search", scoped_search),
    ("only_matching", only_matching),
    ("count", count),
    ("count_matches", count_matches),
    ("files_with_matches", files_with_matches),
    ("files_without_match", files_without_match),
    ("max_count_1", max_count_1),
];

pub fn find_scenario(name: &str) -> Option<Scenario> {
    for (n, f) in ALL_SCENARIOS {
        if *n == name {
            return Some(f());
        }
    }
    None
}

pub fn list_scenario_names() {
    for (name, _) in ALL_SCENARIOS {
        println!("{name}");
    }
}

pub fn scenario_names_joined() -> String {
    ALL_SCENARIOS
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join("|")
}
