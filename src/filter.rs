//! Provides a rudimentary filter layer that can be used to selectively enable progress bars on a
//! per-span level.
use std::marker::PhantomData;

use tracing_core::Subscriber;
use tracing_subscriber::layer::Filter;

/// A filter that filters based on the presence of a field with the name of either
/// "indicatif.pb_show" or "indicatif.pb_hide" on the span.
///
/// The value for this field is irrelevant and not factored in to the filtering (this is due to
/// tracing not making field values available in the `on_new_span` method).
///
/// If both "indicatif.pb_show" and "indicatif.pb_hide" are present, the behavior is to show a
/// progress bar.
pub struct IndicatifFilter<S> {
    show_progress_bars_by_default: bool,
    subscriber: PhantomData<S>,
}

impl<S: Subscriber> IndicatifFilter<S> {
    /// Constructs the filter.
    ///
    /// If "indicatif.pb_show" or "indicatif.pb_hide" are not present as a field on the span,
    /// then the value of `show_progress_bars_by_default` is used; i.e. if
    /// `show_progress_bars_by_default` is `false`, then progress bars are not shown for spans by
    /// default.
    pub fn new(show_progress_bars_by_default: bool) -> Self {
        Self {
            show_progress_bars_by_default,
            subscriber: PhantomData,
        }
    }
}

impl<S: Subscriber> Filter<S> for IndicatifFilter<S> {
    fn enabled(
        &self,
        meta: &tracing::Metadata<'_>,
        _: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        if !meta.is_span() {
            return false;
        }

        if meta.fields().field("indicatif.pb_show").is_some() {
            return true;
        }

        if meta.fields().field("indicatif.pb_hide").is_some() {
            return false;
        }

        self.show_progress_bars_by_default
    }
}
