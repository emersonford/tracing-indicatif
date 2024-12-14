//! Helpers to modify a progress bar associated with a given span.
use indicatif::ProgressStyle;
use tracing::Span;

use crate::IndicatifSpanContext;
use crate::WithContext;

// TODO(emersonford): expose stderr/stdout writers in span ext

fn apply_to_indicatif_span(span: &Span, f: impl FnMut(&mut IndicatifSpanContext)) {
    span.with_subscriber(|(id, subscriber)| {
        if let Some(get_context) = subscriber.downcast_ref::<WithContext>() {
            get_context.with_context(subscriber, id, f);
        }
    });
}

/// Utilities to modify the progress bar associated to tracing [`Span`]'s (if one exists).
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
/// [`IndicatifLayer`](crate::IndicatifLayer) was not registered with the tracing subscriber,
/// or if this span was filtered for the registered `IndicatifLayer`. Because of this behavior, you
/// can "safely" call these methods inside of non-CLI contexts as these methods will gracefully
/// do nothing if you have not enabled progress bars for your tracing spans.
pub trait IndicatifSpanExt {
    /// Sets the [`ProgressStyle`] of the progress bar associated with this span.
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
    /// [`set_length`](indicatif::ProgressBar::set_length).
    fn pb_set_length(&self, len: u64);

    /// Sets the position of the progress bar for this span. See
    /// [`set_position`](indicatif::ProgressBar::set_position).
    ///
    /// WARNING: you should call [`Self::pb_set_length`] at least once before calling this method, or you
    /// may see a buggy progress bar.
    fn pb_set_position(&self, pos: u64);

    /// Increments the position of the progress bar for this span. See
    /// [`inc`](indicatif::ProgressBar::inc).
    ///
    /// WARNING: you should call [`Self::pb_set_length`] at least once before calling this method, or you
    /// may see a buggy progress bar.
    fn pb_inc(&self, delta: u64);

    /// Increments the length of the progress bar for this span. See
    /// [`inc_length`](indicatif::ProgressBar::inc_length).
    ///
    /// Has no effect if [`Self::pb_set_length`] has not been called at least once.
    fn pb_inc_length(&self, delta: u64);

    /// Sets the message of the progress bar for this span. See
    /// [`set_message`](indicatif::ProgressBar::set_message).
    fn pb_set_message(&self, msg: &str);

    /// Trigger a recalculation of the progress bar state. See
    /// [`tick`](indicatif::ProgressBar::tick).
    ///
    /// Has no effect if the progress bar for this span is not active.
    fn pb_tick(&self);

    /// Finish the progress bar
    fn pb_finish_clear(&self);
}

impl IndicatifSpanExt for Span {
    fn pb_set_style(&self, style: &ProgressStyle) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            // Cloning the `ProgressStyle` is necessary to make this `FnMut` :(
            indicatif_ctx.set_progress_bar_style(style.clone());
        });
    }

    fn pb_start(&self) {
        let _ = self.enter();
    }

    fn pb_set_length(&self, len: u64) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            indicatif_ctx.set_progress_bar_length(len);
        });
    }

    fn pb_set_position(&self, pos: u64) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            indicatif_ctx.set_progress_bar_position(pos);
        });
    }

    fn pb_inc(&self, pos: u64) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            indicatif_ctx.inc_progress_bar_position(pos);
        });
    }

    fn pb_inc_length(&self, delta: u64) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            indicatif_ctx.inc_progress_bar_length(delta);
        });
    }

    fn pb_set_message(&self, msg: &str) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            indicatif_ctx.set_progress_bar_message(msg.to_string());
        });
    }

    fn pb_tick(&self) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            indicatif_ctx.progress_bar_tick();
        });
    }

    fn pb_finish_clear(&self) {
        apply_to_indicatif_span(self, |indicatif_ctx| {
            indicatif_ctx.progress_bar_finish_clear();
        });
    }
}
