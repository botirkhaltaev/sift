pub(super) mod json;
pub(super) mod result;
pub(super) mod standard;
pub(super) mod summary;

use std::sync::atomic::AtomicBool;

use crate::format::sink::result::FileResult;
use sift_core::grep::Input;

pub(super) trait InputPrinter {
    fn report(&mut self, input: &Input<'_>, stop: &AtomicBool) -> FileResult;
}
