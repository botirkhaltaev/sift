use regex_syntax::ast::parse::Parser as AstParser;
use regex_syntax::hir::literal::{ExtractKind, Extractor};
use regex_syntax::hir::{self, Hir};

use crate::candidates::CandidateSpec;

use super::config::Config;

impl Config {
    /// Extract literal byte arms from a query spec.
    /// Returns `None` if no usable literals for this N-gram width can be extracted.
    pub(crate) fn extract_literal_arms(self, query: &CandidateSpec<'_>) -> Option<Vec<Vec<u8>>> {
        if query.invert_match() {
            return None;
        }
        let width = self.width().get();
        let mut literal_arms: Vec<Vec<u8>> = Vec::new();
        for p in query.patterns {
            let arms = if query.fixed_strings() {
                Self::fixed_string_literals(p.as_bytes(), query.case_insensitive())
            } else {
                Self::plan_pattern(
                    p.as_str(),
                    query.case_insensitive(),
                    query.word_regexp(),
                    query.line_regexp(),
                    width,
                )?
            };
            for lit in arms {
                if lit.len() < width {
                    return None;
                }
                literal_arms.push(lit);
            }
        }
        if literal_arms.is_empty() {
            return None;
        }
        Some(literal_arms)
    }

    fn plan_pattern(
        pattern: &str,
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
        width: usize,
    ) -> Option<Vec<Vec<u8>>> {
        let hir = Self::build_configured_hir(pattern, case_insensitive)?;
        let shaped = Self::shape_hir(hir, word_regexp, line_regexp);
        let lits = Self::extract_literals(&shaped, width);
        if lits.is_empty() { None } else { Some(lits) }
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

    fn shape_hir(hir: Hir, word_regexp: bool, line_regexp: bool) -> Hir {
        if line_regexp {
            Self::wrap_line(hir)
        } else if word_regexp {
            Self::wrap_word(hir)
        } else {
            hir
        }
    }

    fn wrap_word(hir: Hir) -> Hir {
        Hir::concat(vec![
            Hir::look(hir::Look::WordStartHalfUnicode),
            hir,
            Hir::look(hir::Look::WordEndHalfUnicode),
        ])
    }

    fn wrap_line(hir: Hir) -> Hir {
        Hir::concat(vec![
            Hir::look(hir::Look::StartLF),
            hir,
            Hir::look(hir::Look::EndLF),
        ])
    }

    fn extract_literals(hir: &Hir, width: usize) -> Vec<Vec<u8>> {
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

        Self::pick_better_lits(lits_prefix, lits_suffix, width)
    }

    fn pick_better_lits(
        lits_a: Option<&[regex_syntax::hir::literal::Literal]>,
        lits_b: Option<&[regex_syntax::hir::literal::Literal]>,
        width: usize,
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
            if bytes.len() >= width {
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
}
