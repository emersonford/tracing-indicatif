//! Helpers to modify a progress bar associated with a given span.
use indicatif::ProgressStyle;
use tracing::Span;

use crate::IndicatifSpanContext;
use crate::WithContext;

// TODO(emersonford): add more progress bar mutation methods

fn apply_to_indicatif_span(span: &Span, f: impl FnMut(&mut IndicatifSpanContext)) {
    span.with_subscriber(|(id, subscriber)| {
        if let Some(get_context) = subscriber.downcast_ref::<WithContext>() {
            get_context.with_context(subscriber, id, f);
        }
    });
}

/// Utilities to modify the progress bar associated to tracing [Span]'s (if one exists).
///
/// For example, you can call these on the current Span:
/// ```
/// use tracing_indicatif::span_ext::IndicatifSpanExt;
/// use indicatif::ProgressStyle;
///
/// tracing::Span::current().pb_set_style(&ProgressStyle::default_spinner());
/// ```
///
/// NOTE: These methods will silently have no effect if a
/// [IndicatifLayer](crate::IndicatifLayer) was not registered with the tracing subscriber,
/// or if this span was filtered for the registered `IndicatifLayer`. Because of this behavior, you
/// can "safely" call these methods inside of non-CLI contexts as these methods will gracefully
/// do nothing if you have not enabled progress bars for your tracing spans.
pub trait IndicatifSpanExt {
    /// Sets the [ProgressStyle] of the progress bar associated with this span.
    ///
    /// If this span has not yet been entered, this will be the progress style the progress bar for
    /// this span uses when the span is entered for the first time. If this span has been entered,
    /// this will update the existing progress bar's style.
    fn pb_set_style(&self, style: &ProgressStyle);

    /// Briefly enters the span, which starts the progress bar for the span.
    ///
    /// Has no effect if the span has already been entered before.
    fn pb_start(&self);

    /// Sets the length of the progress bar for this span. See
    /// [set_length](indicatif::ProgressBar::set_length).
    ///
    /// Has no effect if the span has not been entered at least once.
    fn pb_set_length(&self, len: u64);

    /// Sets the position of the progress bar for this span. See
    /// [set_position](indicatif::ProgressBar::set_position).
    ///
    /// Has no effect if the span has not been entered at least once.
    fn pb_set_position(&self, pos: u64);
}

impl IndicatifSpanExt for Span {
    fn pb_set_style(&self, style: &ProgressStyle) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            // Cloning the `ProgressStyle` is necessary to make this `FnMut` :(
            if let Some(ref pb) = indicatif_ctx.progress_bar {
                // We have a visible progress bar, so update in place.
                pb.set_style(indicatif_ctx.add_keys_to_style(style.clone()));
            } else {
                indicatif_ctx.init_progress_style =
                    Some(indicatif_ctx.add_keys_to_style(style.clone()));
            }
        });
    }

    fn pb_start(&self) {
        let _ = self.enter();
    }

    fn pb_set_length(&self, len: u64) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            if let Some(ref pb) = indicatif_ctx.progress_bar {
                pb.set_length(len);
            }
        });
    }

    fn pb_set_position(&self, pos: u64) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            if let Some(ref pb) = indicatif_ctx.progress_bar {
                pb.set_position(pos);
            }
        });
    }
}
