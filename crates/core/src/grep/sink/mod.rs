pub(super) mod json;
pub(super) mod result;
pub(super) mod standard;
pub(super) mod style;
pub(super) mod summary;

use std::sync::atomic::AtomicBool;

use crate::grep::input::GrepInput;
use crate::grep::sink::result::FileResult;

pub(super) trait FileReporter {
    fn report(&mut self, input: &GrepInput<'_>, stop: &AtomicBool) -> FileResult;
}
