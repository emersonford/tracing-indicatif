//! Provides some general tracing utilities that are useful for this crate, for example a utility
//! to filter "indicatif.pb_show" and "indicatif.pb_hide" from printing spans.
use std::fmt;

use tracing::field::Visit;
use tracing_core::Field;
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::{
    field::{MakeVisitor, VisitFmt, VisitOutput},
    fmt::{format::Writer, FormatFields},
};

/// Wraps around an existing struct that impls [FormatFields], but allows for filtering specific
/// fields from spans or events.
pub struct FilteredFormatFields<Format, Filter> {
    format: Format,
    filter: Filter,
}

impl<'writer, Format, Filter> FilteredFormatFields<Format, Filter>
where
    Format: MakeVisitor<Writer<'writer>>,
    Format::Visitor: VisitFmt + VisitOutput<fmt::Result>,
    Filter: Clone + Fn(&Field) -> bool,
{
    /// Wraps around an existing struct that impls [FormatFields], but filters out any fields which
    /// returns _false_ when passed into `filter`.
    pub fn new(format: Format, filter: Filter) -> Self {
        Self { format, filter }
    }
}

impl<'writer, Format, Filter> FormatFields<'writer> for FilteredFormatFields<Format, Filter>
where
    Format: MakeVisitor<Writer<'writer>>,
    Format::Visitor: VisitFmt + VisitOutput<fmt::Result>,
    Filter: Clone + Fn(&Field) -> bool,
{
    fn format_fields<R: RecordFields>(
        &self,
        writer: Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        let mut v =
            FilteredFormatFieldsVisitor::new(self.format.make_visitor(writer), self.filter.clone());
        fields.record(&mut v);
        v.finish()?;

        Ok(())
    }
}

struct FilteredFormatFieldsVisitor<Visitor, Filter> {
    visitor: Visitor,
    filter: Filter,
}

impl<Visitor, Filter> FilteredFormatFieldsVisitor<Visitor, Filter> {
    fn new(visitor: Visitor, filter: Filter) -> Self {
        Self { visitor, filter }
    }
}

impl<Visitor, Filter> Visit for FilteredFormatFieldsVisitor<Visitor, Filter>
where
    Visitor: Visit,
    Filter: Fn(&Field) -> bool,
{
    fn record_debug(&mut self, field: &tracing_core::Field, value: &dyn std::fmt::Debug) {
        if (self.filter)(field) {
            self.visitor.record_debug(field, value);
        }
    }

    fn record_f64(&mut self, field: &tracing_core::Field, value: f64) {
        if (self.filter)(field) {
            self.visitor.record_f64(field, value);
        }
    }

    fn record_i64(&mut self, field: &tracing_core::Field, value: i64) {
        if (self.filter)(field) {
            self.visitor.record_i64(field, value);
        }
    }

    fn record_u64(&mut self, field: &tracing_core::Field, value: u64) {
        if (self.filter)(field) {
            self.visitor.record_u64(field, value);
        }
    }

    fn record_str(&mut self, field: &tracing_core::Field, value: &str) {
        if (self.filter)(field) {
            self.visitor.record_str(field, value);
        }
    }

    fn record_i128(&mut self, field: &tracing_core::Field, value: i128) {
        if (self.filter)(field) {
            self.visitor.record_i128(field, value);
        }
    }

    fn record_u128(&mut self, field: &tracing_core::Field, value: u128) {
        if (self.filter)(field) {
            self.visitor.record_u128(field, value);
        }
    }

    fn record_bool(&mut self, field: &tracing_core::Field, value: bool) {
        if (self.filter)(field) {
            self.visitor.record_bool(field, value);
        }
    }

    fn record_error(
        &mut self,
        field: &tracing_core::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        if (self.filter)(field) {
            self.visitor.record_error(field, value);
        }
    }
}

impl<Visitor, Filter> VisitOutput<fmt::Result> for FilteredFormatFieldsVisitor<Visitor, Filter>
where
    Visitor: VisitOutput<fmt::Result>,
    Filter: Fn(&Field) -> bool,
{
    fn finish(self) -> fmt::Result {
        self.visitor.finish()
    }
}

impl<Visitor, Filter> VisitFmt for FilteredFormatFieldsVisitor<Visitor, Filter>
where
    Visitor: VisitFmt,
    Filter: Fn(&Field) -> bool,
{
    fn writer(&mut self) -> &mut dyn fmt::Write {
        self.visitor.writer()
    }
}
