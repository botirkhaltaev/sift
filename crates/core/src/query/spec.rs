#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct QuerySpec<'a> {
    pub patterns: &'a [String],
    pub fixed_strings: bool,
    pub case_insensitive: bool,
    pub word_regexp: bool,
    pub line_regexp: bool,
    pub invert_match: bool,
}
