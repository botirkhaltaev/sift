use std::time::Instant;

use sift_core::search::Report;
use sift_core::{Grep, GrepRequest};

use crate::format::collection::PrintExtras;
use crate::format::event::EventRenderer;
use crate::format::output::PrintSpec;
use crate::format::output::style::PrintSeparators;

pub struct SearchPrinter;

impl SearchPrinter {
    /// Run the high-level grep pipeline and write formatted output to stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if search or output formatting fails.
    pub fn print_grep(
        grep: &Grep<'_>,
        request: GrepRequest<'_>,
        print_spec: PrintSpec,
        separators: &PrintSeparators,
        extras: PrintExtras,
    ) -> sift_core::Result<Report> {
        let started = Instant::now();
        let context_requested =
            request.query.options().before_context > 0 || request.query.options().after_context > 0;
        let binary_mode = request.query.options().binary_mode;
        let mut renderer = EventRenderer::new(
            print_spec,
            separators,
            extras,
            started,
            binary_mode,
            context_requested,
        );
        let mut report = grep.stream(request, &mut renderer)?;
        renderer.finish(&mut report)?;
        Ok(report)
    }
}
