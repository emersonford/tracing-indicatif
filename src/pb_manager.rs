use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use indicatif::style::ProgressStyle;
use indicatif::MultiProgress;
use indicatif::ProgressBar;
use indicatif::ProgressDrawTarget;
use indicatif::ProgressState;
use tracing_core::span;
use tracing_core::Subscriber;
use tracing_subscriber::layer;
use tracing_subscriber::registry::LookupSpan;

use crate::IndicatifSpanContext;

pub(crate) struct ProgressBarManager {
    pub(crate) mp: MultiProgress,
    active_progress_bars: u64,
    max_progress_bars: u64,
    // This is used in the footer progress bar and tracks the actual number of pending progress
    // bars.
    pending_progress_bars: Arc<AtomicUsize>,
    // The `.len()` of this may differ from `pending_progress_bars`. If a span closes before its
    // progress bar is ever un-hidden, we decrement `pending_progress_bars` but won't clean the
    // span entry up from this `VecDeque` for performance reasons. Instead, whenever we do un-hide
    // a progress bar, we'll "garbage collect" closed spans from this then.
    pending_spans: VecDeque<span::Id>,
    // If this is `None`, a footer will never be shown.
    footer_pb: Option<ProgressBar>,
}

impl ProgressBarManager {
    pub(crate) fn new(
        max_progress_bars: u64,
        footer_progress_style: Option<ProgressStyle>,
    ) -> Self {
        let pending_progress_bars = Arc::new(AtomicUsize::new(0));

        Self {
            mp: {
                let mp = MultiProgress::new();
                mp.set_draw_target(ProgressDrawTarget::stderr_with_hz(20));

                mp
            },
            active_progress_bars: 0,
            max_progress_bars,
            pending_progress_bars: pending_progress_bars.clone(),
            pending_spans: VecDeque::new(),
            footer_pb: footer_progress_style.map(|style| {
                ProgressBar::hidden().with_style(style.with_key(
                    "pending_progress_bars",
                    move |_: &ProgressState, writer: &mut dyn std::fmt::Write| {
                        let _ = write!(
                            writer,
                            "{}",
                            pending_progress_bars.load(std::sync::atomic::Ordering::Acquire)
                        );
                    },
                ))
            }),
        }
    }

    fn decrement_pending_pb(&mut self) {
        let prev_val = self
            .pending_progress_bars
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);

        // If this span was the last one pending, clear the footer (if it was active).
        if prev_val == 1 {
            debug_assert!(
                self.footer_pb
                    .as_ref()
                    .map(|pb| !pb.is_hidden())
                    .unwrap_or(true),
                "footer progress bar was hidden despite there being pending progress bars"
            );

            if let Some(footer_pb) = self.footer_pb.as_ref() {
                // Appears to have broken with
                // https://github.com/console-rs/indicatif/pull/648
                // self.mp.set_move_cursor(false);
                footer_pb.finish_and_clear();
                self.mp.remove(footer_pb);
                footer_pb.disable_steady_tick();
            }
        }
    }

    fn add_pending_pb(&mut self, span_id: &span::Id) {
        let prev_val = self
            .pending_progress_bars
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.pending_spans.push_back(span_id.clone());

        if prev_val == 0 {
            debug_assert!(
                self.footer_pb
                    .as_ref()
                    .map(|pb| pb.is_hidden())
                    .unwrap_or(true),
                "footer progress bar was not hidden despite there being no pending progress bars"
            );

            // Show the footer progress bar.
            if let Some(footer_pb) = self.footer_pb.as_ref() {
                self.mp.add(footer_pb.clone());
                footer_pb.enable_steady_tick(Duration::from_millis(100));
                // Appears to have broken with
                // https://github.com/console-rs/indicatif/pull/648
                // self.mp.set_move_cursor(true);
            }
        }
    }

    pub(crate) fn show_progress_bar(
        &mut self,
        pb_span_ctx: &mut IndicatifSpanContext,
        span_id: &span::Id,
    ) {
        if self.active_progress_bars < self.max_progress_bars {
            let pb = match pb_span_ctx.parent_progress_bar {
                // TODO(emersonford): fix span ordering in progress bar, because we use
                // `insert_after`, we end up showing the child progress bars in reverse order.
                Some(ref parent_pb) => self
                    .mp
                    .insert_after(parent_pb, pb_span_ctx.progress_bar.take().unwrap()),
                None => {
                    if self
                        .footer_pb
                        .as_ref()
                        .map(|footer_pb| !footer_pb.is_hidden())
                        .unwrap_or(false)
                    {
                        self.mp
                            .insert_from_back(1, pb_span_ctx.progress_bar.take().unwrap())
                    } else {
                        self.mp.add(pb_span_ctx.progress_bar.take().unwrap())
                    }
                }
            };

            self.active_progress_bars += 1;

            pb.enable_steady_tick(Duration::from_millis(100));
            pb_span_ctx.progress_bar = Some(pb);
        } else {
            self.add_pending_pb(span_id);
        }
    }

    pub(crate) fn finish_progress_bar<S>(
        &mut self,
        pb_span_ctx: &mut IndicatifSpanContext,
        ctx: &layer::Context<'_, S>,
    ) where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let Some(pb) = pb_span_ctx.progress_bar.take() else {
            // Span was never entered.
            return;
        };

        // The span closed before we had a chance to show its progress bar.
        if pb.is_hidden() {
            self.decrement_pending_pb();
            return;
        }

        // This span had an active/shown progress bar.
        pb.finish_and_clear();
        self.mp.remove(&pb);
        self.active_progress_bars -= 1;

        loop {
            let Some(span_id) = self.pending_spans.pop_front() else {
                break;
            };

            match ctx.span(&span_id) {
                Some(next_eligible_span) => {
                    let mut ext = next_eligible_span.extensions_mut();
                    let indicatif_span_ctx = ext
                        .get_mut::<IndicatifSpanContext>()
                        .expect("No IndicatifSpanContext found; this is a bug");

                    // It possible `on_close` has been called on a span but it has not yet been
                    // removed from `ctx.span` (e.g., tracing may still be iterating through each
                    // layer's `on_close` method and cannot remove the span from the registry until
                    // it has finished `on_close` for each layer). So we may successfully fetch the
                    // span, despite having closed out its progress bar.
                    if indicatif_span_ctx.progress_bar.is_none() {
                        continue;
                    }

                    self.decrement_pending_pb();
                    self.show_progress_bar(indicatif_span_ctx, &span_id);
                    break;
                }
                None => {
                    // Span was closed earlier, we "garbage collect" it from the queue here.
                    continue;
                }
            }
        }
    }
}
