use std::io::IsTerminal;

use crate::format::output::format::ColumnLimit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilenameMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Never,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputBuffering {
    #[default]
    Auto,
    Line,
    Block,
}

pub use sift_core::grep::PathDisplay;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct LineStyleFlags: u8 {
        const HEADING     = 1 << 0;
        const LINE_NUMBER = 1 << 1;
        const BYTE_OFFSET = 1 << 2;
        const TRIM        = 1 << 3;
        const COLUMN      = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrintLineStyle {
    pub filename_mode: FilenameMode,
    pub flags: LineStyleFlags,
    pub path_display: PathDisplay,
    pub columns: Option<ColumnLimit>,
}

impl PrintLineStyle {
    #[must_use]
    pub const fn heading(self) -> bool {
        self.flags.contains(LineStyleFlags::HEADING)
    }

    #[must_use]
    pub const fn line_number(self) -> bool {
        self.flags.contains(LineStyleFlags::LINE_NUMBER)
    }

    #[must_use]
    pub const fn byte_offset(self) -> bool {
        self.flags.contains(LineStyleFlags::BYTE_OFFSET)
    }

    #[must_use]
    pub const fn trim(self) -> bool {
        self.flags.contains(LineStyleFlags::TRIM)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordTerminator {
    #[default]
    Newline,
    Nul,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrintRecordStyle {
    pub terminator: RecordTerminator,
    pub color: ColorChoice,
    pub path_separator: Option<u8>,
    pub colors: ColorSpecs,
    pub hyperlink: HyperlinkFormat,
    pub hyperlink_host: Option<String>,
    pub buffering: OutputBuffering,
}

impl PrintRecordStyle {
    #[must_use]
    pub fn should_color(&self) -> bool {
        match self.color {
            ColorChoice::Never => false,
            ColorChoice::Always => true,
            ColorChoice::Auto => {
                std::io::stdout().is_terminal()
                    && std::env::var_os("NO_COLOR").is_none()
                    && std::env::var_os("TERM").is_none_or(|term| term != "dumb")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpecs {
    pub path: AnsiStyle,
    pub line: AnsiStyle,
    pub column: AnsiStyle,
    pub matched: AnsiStyle,
    pub highlight: AnsiStyle,
}

impl Default for ColorSpecs {
    fn default() -> Self {
        Self {
            #[cfg(unix)]
            path: AnsiStyle::new().fg(AnsiColor::Basic(AnsiBasicColor::Magenta)),
            #[cfg(windows)]
            path: AnsiStyle::new().fg(AnsiColor::Basic(AnsiBasicColor::Cyan)),
            line: AnsiStyle::new().fg(AnsiColor::Basic(AnsiBasicColor::Green)),
            column: AnsiStyle::new().fg(AnsiColor::Basic(AnsiBasicColor::Green)),
            matched: AnsiStyle::new()
                .fg(AnsiColor::Basic(AnsiBasicColor::Red))
                .bold(true),
            highlight: AnsiStyle::new(),
        }
    }
}

impl ColorSpecs {
    /// # Errors
    ///
    /// Returns an error when a user color specification is not ripgrep-compatible.
    pub fn from_specs(specs: &[String]) -> Result<Self, String> {
        let mut colors = Self::default();
        for spec in specs {
            colors.apply(spec)?;
        }
        Ok(colors)
    }

    fn apply(&mut self, spec: &str) -> Result<(), String> {
        let mut parts = spec.split(':');
        let target = parts.next().ok_or_else(|| invalid_color_format(spec))?;
        let attr = parts.next().ok_or_else(|| invalid_color_format(spec))?;
        let value = parts.next();
        if parts.next().is_some()
            || (attr != "none" && value.is_none())
            || (attr == "none" && value.is_some())
        {
            return Err(invalid_color_format(spec));
        }
        let style = match target {
            "path" => &mut self.path,
            "line" => &mut self.line,
            "column" => &mut self.column,
            "match" => &mut self.matched,
            "highlight" => &mut self.highlight,
            other => {
                return Err(format!(
                    "unrecognized output type '{other}'. Choose from: path, line, column, match, highlight."
                ));
            }
        };
        match attr {
            "none" => *style = AnsiStyle::new(),
            "fg" => style.foreground = Some(parse_ansi_color(value.unwrap())?),
            "bg" => style.background = Some(parse_ansi_color(value.unwrap())?),
            "style" => apply_style(style, value.unwrap())?,
            other => {
                return Err(format!(
                    "unrecognized spec type '{other}'. Choose from: fg, bg, style, none."
                ));
            }
        }
        Ok(())
    }
}

fn invalid_color_format(spec: &str) -> String {
    format!(
        "invalid color spec format: '{spec}'. Valid format is '(path|line|column|match|highlight):(fg|bg|style):(value)'."
    )
}

fn apply_style(style: &mut AnsiStyle, value: &str) -> Result<(), String> {
    match value {
        "bold" => style.bold = Some(true),
        "nobold" => style.bold = Some(false),
        "intense" => style.intense = Some(true),
        "nointense" => style.intense = Some(false),
        "underline" => style.underline = Some(true),
        "nounderline" => style.underline = Some(false),
        "italic" => style.italic = Some(true),
        "noitalic" => style.italic = Some(false),
        other => {
            return Err(format!(
                "unrecognized style attribute '{other}'. Choose from: nobold, bold, nointense, intense, nounderline, underline, noitalic, italic."
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnsiStyle {
    foreground: Option<AnsiColor>,
    background: Option<AnsiColor>,
    bold: Option<bool>,
    intense: Option<bool>,
    underline: Option<bool>,
    italic: Option<bool>,
}

impl AnsiStyle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            foreground: None,
            background: None,
            bold: None,
            intense: None,
            underline: None,
            italic: None,
        }
    }

    #[must_use]
    pub const fn fg(mut self, color: AnsiColor) -> Self {
        self.foreground = Some(color);
        self
    }

    #[must_use]
    pub const fn bold(mut self, yes: bool) -> Self {
        self.bold = Some(yes);
        self
    }

    #[must_use]
    pub const fn is_plain(self) -> bool {
        self.foreground.is_none()
            && self.background.is_none()
            && self.bold.is_none()
            && self.intense.is_none()
            && self.underline.is_none()
            && self.italic.is_none()
    }

    pub fn write_start(self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[0m");
        let mut first = true;
        out.extend_from_slice(b"\x1b[");
        for code in self.sgr_codes() {
            if !first {
                out.push(b';');
            }
            first = false;
            out.extend_from_slice(code.as_bytes());
        }
        out.push(b'm');
    }

    fn sgr_codes(self) -> Vec<String> {
        let mut codes = Vec::new();
        if let Some(bold) = self.bold {
            codes.push(if bold { "1" } else { "22" }.to_string());
        }
        if let Some(intense) = self.intense {
            codes.push(if intense { "1" } else { "22" }.to_string());
        }
        if let Some(underline) = self.underline {
            codes.push(if underline { "4" } else { "24" }.to_string());
        }
        if let Some(italic) = self.italic {
            codes.push(if italic { "3" } else { "23" }.to_string());
        }
        if let Some(color) = self.foreground {
            color.push_sgr(false, &mut codes);
        }
        if let Some(color) = self.background {
            color.push_sgr(true, &mut codes);
        }
        if codes.is_empty() {
            codes.push("0".to_string());
        }
        codes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Basic(AnsiBasicColor),
    Fixed(u8),
    Rgb(u8, u8, u8),
}

impl AnsiColor {
    fn push_sgr(self, background: bool, codes: &mut Vec<String>) {
        match self {
            Self::Basic(color) => codes.push(color.sgr(background).to_string()),
            Self::Fixed(n) => {
                codes.push(if background { "48" } else { "38" }.to_string());
                codes.push("5".to_string());
                codes.push(n.to_string());
            }
            Self::Rgb(r, g, b) => {
                codes.push(if background { "48" } else { "38" }.to_string());
                codes.push("2".to_string());
                codes.push(r.to_string());
                codes.push(g.to_string());
                codes.push(b.to_string());
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiBasicColor {
    Black,
    Blue,
    Green,
    Red,
    Cyan,
    Magenta,
    Yellow,
    White,
}

impl AnsiBasicColor {
    const fn sgr(self, background: bool) -> u8 {
        let base = if background { 40 } else { 30 };
        base + match self {
            Self::Black => 0,
            Self::Red => 1,
            Self::Green => 2,
            Self::Yellow => 3,
            Self::Blue => 4,
            Self::Magenta => 5,
            Self::Cyan => 6,
            Self::White => 7,
        }
    }
}

fn parse_ansi_color(value: &str) -> Result<AnsiColor, String> {
    match value {
        "black" => Ok(AnsiColor::Basic(AnsiBasicColor::Black)),
        "blue" => Ok(AnsiColor::Basic(AnsiBasicColor::Blue)),
        "green" => Ok(AnsiColor::Basic(AnsiBasicColor::Green)),
        "red" => Ok(AnsiColor::Basic(AnsiBasicColor::Red)),
        "cyan" => Ok(AnsiColor::Basic(AnsiBasicColor::Cyan)),
        "magenta" => Ok(AnsiColor::Basic(AnsiBasicColor::Magenta)),
        "yellow" => Ok(AnsiColor::Basic(AnsiBasicColor::Yellow)),
        "white" => Ok(AnsiColor::Basic(AnsiBasicColor::White)),
        _ => parse_extended_color(value),
    }
}

fn parse_extended_color(value: &str) -> Result<AnsiColor, String> {
    let values: Vec<&str> = value.split(',').collect();
    match values.as_slice() {
        [n] => parse_color_component(n).map(AnsiColor::Fixed),
        [r, g, b] => Ok(AnsiColor::Rgb(
            parse_color_component(r)?,
            parse_color_component(g)?,
            parse_color_component(b)?,
        )),
        _ => Err(format!("unrecognized color '{value}'")),
    }
}

fn parse_color_component(value: &str) -> Result<u8, String> {
    value.strip_prefix("0x").map_or_else(
        || {
            value
                .parse::<u8>()
                .map_err(|_| format!("unrecognized color '{value}'"))
        },
        |hex| u8::from_str_radix(hex, 16).map_err(|_| format!("unrecognized color '{value}'")),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HyperlinkFormat {
    template: String,
}

impl HyperlinkFormat {
    /// # Errors
    ///
    /// Returns an error when the format has invalid braces or variables.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        let template = match value {
            "cursor" => "cursor://file{path}:{line}:{column}",
            "default" | "file" => "file://{host}{path}",
            "grep+" => "grep+://{path}:{line}",
            "kitty" => "file://{host}{path}#{line}",
            "macvim" => "mvim://open?url=file://{path}&line={line}&column={column}",
            "none" => "",
            "textmate" => "txmt://open?url=file://{path}&line={line}&column={column}",
            "vscode" => "vscode://file{path}:{line}:{column}",
            "vscode-insiders" => "vscode-insiders://file{path}:{line}:{column}",
            "vscodium" => "vscodium://file{path}:{line}:{column}",
            custom => custom,
        };
        validate_hyperlink_template(template)?;
        Ok(Self {
            template: template.to_string(),
        })
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.template.is_empty()
    }

    #[must_use]
    pub fn render(&self, values: HyperlinkValues<'_>) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut rendered = self.template.clone();
        rendered = rendered.replace("{path}", values.path);
        rendered = rendered.replace("{line}", &values.line.unwrap_or(0).to_string());
        rendered = rendered.replace("{column}", &values.column.unwrap_or(0).to_string());
        rendered = rendered.replace("{host}", values.host.unwrap_or(""));
        Some(rendered)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HyperlinkValues<'a> {
    pub path: &'a str,
    pub line: Option<u64>,
    pub column: Option<usize>,
    pub host: Option<&'a str>,
}

fn validate_hyperlink_template(template: &str) -> Result<(), String> {
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == '}' {
                        closed = true;
                        break;
                    }
                    name.push(next);
                }
                if !closed {
                    return Err("invalid hyperlink format: unmatched '{'".to_string());
                }
                if !matches!(name.as_str(), "path" | "line" | "column" | "host") {
                    return Err(format!("unrecognized hyperlink variable '{{{name}}}'"));
                }
            }
            '}' => return Err("invalid hyperlink format: unmatched '}'".to_string()),
            _ => {}
        }
    }
    Ok(())
}

impl RecordTerminator {
    pub fn write_to(&self, out: &mut Vec<u8>) {
        match self {
            Self::Nul => out.push(0),
            Self::Newline => out.push(b'\n'),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintSeparators {
    pub context_separator: Option<Vec<u8>>,
    pub field_match_separator: Vec<u8>,
    pub field_context_separator: Vec<u8>,
}

impl Default for PrintSeparators {
    fn default() -> Self {
        Self {
            context_separator: Some(b"--".to_vec()),
            field_match_separator: b":".to_vec(),
            field_context_separator: b"-".to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_line_style_defaults() {
        let style = PrintLineStyle::default();
        assert!(!style.heading());
        assert!(!style.line_number());
        assert!(!style.byte_offset());
        assert!(!style.trim());
    }

    #[test]
    fn search_record_style_defaults() {
        let style = PrintRecordStyle::default();
        assert!(matches!(style.terminator, RecordTerminator::Newline));
        assert_eq!(style.color, ColorChoice::Auto);
        assert!(style.path_separator.is_none());
    }

    #[test]
    fn search_separators_defaults() {
        let sep = PrintSeparators::default();
        assert_eq!(sep.context_separator, Some(b"--".to_vec()));
        assert_eq!(sep.field_match_separator, b":".to_vec());
        assert_eq!(sep.field_context_separator, b"-".to_vec());
    }
}
