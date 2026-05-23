use regex_syntax::ast::parse::Parser as AstParser;
use regex_syntax::hir::literal::{ExtractKind, Extractor};
use regex_syntax::hir::{self, Hir};

use super::candidate_plan::{Arm, CandidatePlan, TrigramCandidatePlan};
use super::spec::QuerySpec;
use super::trigram::extract_trigrams_from_bytes;

pub struct QueryPlanner;

impl QueryPlanner {
    #[must_use]
    pub fn plan(spec: &QuerySpec<'_>) -> CandidatePlan {
        if spec.invert_match {
            return CandidatePlan::FullScan;
        }
        let mut trigram_arms: Vec<Arm> = Vec::new();
        for p in spec.patterns {
            let arms = if spec.fixed_strings {
                fixed_string_literals(p.as_bytes(), spec.case_insensitive)
            } else {
                match plan_pattern(
                    p.as_str(),
                    spec.case_insensitive,
                    spec.word_regexp,
                    spec.line_regexp,
                ) {
                    Some(a) => a,
                    None => return CandidatePlan::FullScan,
                }
            };
            for lit in arms {
                if lit.len() < 3 {
                    return CandidatePlan::FullScan;
                }
                trigram_arms.push(extract_trigrams_from_bytes(&lit));
            }
        }
        if trigram_arms.is_empty() {
            return CandidatePlan::FullScan;
        }
        CandidatePlan::Trigram(TrigramCandidatePlan { arms: trigram_arms })
    }
}

fn build_configured_hir(pattern: &str, case_insensitive: bool) -> Option<Hir> {
    let ast = AstParser::new().parse(pattern).ok()?;
    let mut builder = regex_syntax::hir::translate::TranslatorBuilder::new();
    builder.unicode(true);
    if case_insensitive {
        builder.case_insensitive(true);
    }
    let mut translator = builder.build();
    let hir = translator.translate(pattern, &ast).ok()?;
    Some(hir)
}

fn wrap_word(hir: Hir, unicode: bool) -> Hir {
    let start_half = if unicode {
        hir::Look::WordStartHalfUnicode
    } else {
        hir::Look::WordStartHalfAscii
    };
    let end_half = if unicode {
        hir::Look::WordEndHalfUnicode
    } else {
        hir::Look::WordEndHalfAscii
    };
    Hir::concat(vec![Hir::look(start_half), hir, Hir::look(end_half)])
}

fn wrap_line(hir: Hir) -> Hir {
    Hir::concat(vec![
        Hir::look(hir::Look::StartLF),
        hir,
        Hir::look(hir::Look::EndLF),
    ])
}

fn shape_hir(hir: Hir, word_regexp: bool, line_regexp: bool) -> Hir {
    if line_regexp {
        wrap_line(hir)
    } else if word_regexp {
        wrap_word(hir, true)
    } else {
        hir
    }
}

fn extract_literals(hir: &Hir) -> Vec<Vec<u8>> {
    let extractor_prefix = Extractor::new();
    let extractor_suffix = {
        let mut e = Extractor::new();
        e.kind(ExtractKind::Suffix);
        e
    };

    let seq_prefix = extractor_prefix.extract(hir);
    let seq_suffix = extractor_suffix.extract(hir);

    let lits_prefix = seq_prefix.literals();
    let lits_suffix = seq_suffix.literals();

    pick_better_lits(lits_prefix, lits_suffix)
}

fn pick_better_lits(
    lits_a: Option<&[regex_syntax::hir::literal::Literal]>,
    lits_b: Option<&[regex_syntax::hir::literal::Literal]>,
) -> Vec<Vec<u8>> {
    fn total_bytes(lits: Option<&[regex_syntax::hir::literal::Literal]>) -> usize {
        lits.map_or(0, |l| l.iter().map(|lit| lit.as_bytes().len()).sum())
    }

    let a_count = lits_a.map_or(0, <[regex_syntax::hir::literal::Literal]>::len);
    let b_count = lits_b.map_or(0, <[regex_syntax::hir::literal::Literal]>::len);
    let a_has = a_count > 0;
    let b_has = b_count > 0;

    let lits = match (a_has, b_has) {
        (true, false) => lits_a,
        (false, true) => lits_b,
        (false, false) => return Vec::new(),
        (true, true) => {
            let a_total = total_bytes(lits_a);
            let b_total = total_bytes(lits_b);
            if a_total >= b_total { lits_a } else { lits_b }
        }
    };

    let lits = match lits {
        Some(l) if !l.is_empty() => l,
        _ => return Vec::new(),
    };

    let mut out = Vec::new();
    for lit in lits {
        let bytes = lit.as_bytes();
        if bytes.len() >= 3 {
            out.push(bytes.to_vec());
        }
    }
    out
}

fn fixed_string_literals(lit: &[u8], case_insensitive: bool) -> Vec<Vec<u8>> {
    if case_insensitive {
        vec![lit.to_ascii_lowercase()]
    } else {
        vec![lit.to_vec()]
    }
}

fn plan_pattern(
    pattern: &str,
    case_insensitive: bool,
    word_regexp: bool,
    line_regexp: bool,
) -> Option<Vec<Vec<u8>>> {
    let hir = build_configured_hir(pattern, case_insensitive)?;
    let shaped = shape_hir(hir, word_regexp, line_regexp);
    let lits = extract_literals(&shaped);
    if lits.is_empty() { None } else { Some(lits) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn narrow(
        patterns: &[String],
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
    ) -> bool {
        let spec = QuerySpec {
            patterns,
            fixed_strings: false,
            case_insensitive,
            word_regexp,
            line_regexp,
            invert_match: false,
        };
        matches!(QueryPlanner::plan(&spec), CandidatePlan::Trigram(_))
    }

    fn full_scan(
        patterns: &[String],
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
    ) -> bool {
        let spec = QuerySpec {
            patterns,
            fixed_strings: false,
            case_insensitive,
            word_regexp,
            line_regexp,
            invert_match: false,
        };
        matches!(QueryPlanner::plan(&spec), CandidatePlan::FullScan)
    }

    #[test]
    fn literal_narrows() {
        assert!(narrow(&["beta".to_string()], false, false, false));
    }

    #[test]
    fn dot_star_full_scan() {
        assert!(full_scan(&[".*".to_string()], false, false, false));
    }

    #[test]
    fn alternation_narrows() {
        assert!(narrow(&[r"foo|bar".to_string()], false, false, false));
    }

    #[test]
    fn word_literal_narrows() {
        assert!(narrow(&["beta".to_string()], false, true, false));
    }

    #[test]
    fn line_regexp_narrows() {
        assert!(narrow(&["beta".to_string()], false, false, true));
    }

    #[test]
    fn case_insensitive_narrows() {
        assert!(narrow(&["beta".to_string()], true, false, false));
    }

    #[test]
    fn required_literal_inside_regex_narrows() {
        assert!(narrow(&["[A-Z]+_RESUME".to_string()], false, false, false));
    }

    #[test]
    fn unicode_class_full_scan() {
        assert!(full_scan(&[r"\p{Greek}".to_string()], false, false, false));
    }

    #[test]
    fn no_literal_full_scan() {
        assert!(full_scan(
            &[r"\w{5}\s+\w{5}".to_string()],
            false,
            false,
            false
        ));
    }

    #[test]
    fn short_literal_full_scan() {
        assert!(full_scan(&["ab".to_string()], false, false, false));
    }

    #[test]
    fn fixed_string_narrows() {
        let spec = QuerySpec {
            patterns: &["beta.gamma".to_string()],
            fixed_strings: true,
            case_insensitive: false,
            word_regexp: false,
            line_regexp: false,
            invert_match: false,
        };
        assert!(matches!(
            QueryPlanner::plan(&spec),
            CandidatePlan::Trigram(_)
        ));
    }
}
