use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use indicatif::MultiProgress;
use indicatif::ProgressBar;
use indicatif::ProgressDrawTarget;
use indicatif::ProgressState;
use indicatif::style::ProgressStyle;
use tracing_core::Subscriber;
use tracing_core::span;
use tracing_subscriber::layer;
use tracing_subscriber::registry::LookupSpan;

use crate::IndicatifSpanContext;

#[derive(Clone)]
struct RequireDefault;

/// Controls how often progress bars are recalculated and redrawn to the terminal.
///
/// This struct must be constructed as
/// ```
/// # use tracing_indicatif::TickSettings;
/// TickSettings {
///     term_draw_hz: 20,
///     default_tick_interval: None,
///     footer_tick_interval: None,
///     ..Default::default()
/// }
/// # ;
/// ```
/// as to ensure forward compatibility.
#[derive(Clone)]
pub struct TickSettings {
    /// The rate at which to draw to the terminal.
    ///
    /// A value of 20 here means indicatif will redraw the terminal state 20 times a second (i.e.
    /// once every 50ms).
    pub term_draw_hz: u8,
    /// The default interval to pass to `enable_steady_tick` for a new progress bar. This controls
    /// how often the progress bar state is recalculated. Defaults to
    /// `Some(Duration::from_millis(100))`.
    ///
    /// Note, this does not control how often the progress bar is actually redrawn, that is
    /// controlled by [`Self::term_draw_hz`].
    ///
    /// Using `None` here will disable steady ticks for your progress bars.
    pub default_tick_interval: Option<Duration>,
    /// The interval to pass to `enable_steady_tick` for the footer progress bar. This controls
    /// how often the footer progress bar state is recalculated. Defaults to `None`.
    ///
    /// Note, this does not control how often the footer progress bar is actually redrawn, that is
    /// controlled by [`Self::term_draw_hz`].
    ///
    /// Using `None` here will disable steady ticks for the footer progress bar. Unless you have a
    /// spinner in your footer, you should set this to `None` as we manually redraw the footer
    /// whenever something changes.
    pub footer_tick_interval: Option<Duration>,
    // Exists solely to require `..Default::default()` at the end of constructing this struct.
    #[doc(hidden)]
    #[allow(private_interfaces)]
    pub require_default: RequireDefault,
}

impl Default for TickSettings {
    fn default() -> Self {
        Self {
            term_draw_hz: 20,
            default_tick_interval: Some(Duration::from_millis(100)),
            footer_tick_interval: None,
            require_default: RequireDefault,
        }
    }
}

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
    tick_settings: TickSettings,
}

impl ProgressBarManager {
    pub(crate) fn new(
        max_progress_bars: u64,
        footer_progress_style: Option<ProgressStyle>,
        tick_settings: TickSettings,
    ) -> Self {
        let mut s = Self {
            mp: {
                let mp = MultiProgress::new();
                mp.set_draw_target(ProgressDrawTarget::stderr_with_hz(
                    tick_settings.term_draw_hz,
                ));

                mp
            },
            active_progress_bars: 0,
            max_progress_bars: 0,
            pending_progress_bars: Arc::new(AtomicUsize::new(0)),
            pending_spans: VecDeque::new(),
            footer_pb: None,
            tick_settings,
        };

        s.set_max_progress_bars(max_progress_bars, footer_progress_style);

        s
    }

    pub(crate) fn set_max_progress_bars(
        &mut self,
        max_progress_bars: u64,
        footer_style: Option<ProgressStyle>,
    ) {
        self.max_progress_bars = max_progress_bars;

        let pending_progress_bars = self.pending_progress_bars.clone();
        self.footer_pb = footer_style.map(move |style| {
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
        });
    }

    pub(crate) fn set_tick_settings(&mut self, tick_settings: TickSettings) {
        self.mp.set_draw_target(ProgressDrawTarget::stderr_with_hz(
            tick_settings.term_draw_hz,
        ));
        self.tick_settings = tick_settings;
    }

    fn decrement_pending_pb(&mut self) {
        let prev_val = self
            .pending_progress_bars
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);

        if let Some(footer_pb) = self.footer_pb.as_ref() {
            // If this span was the last one pending, clear the footer (if it was active).
            if prev_val == 1 {
                debug_assert!(
                    !footer_pb.is_hidden(),
                    "footer progress bar was hidden despite there being pending progress bars"
                );

                if self.tick_settings.footer_tick_interval.is_some() {
                    footer_pb.disable_steady_tick();
                }

                // Appears to have broken with
                // https://github.com/console-rs/indicatif/pull/648
                // self.mp.set_move_cursor(false);
                footer_pb.finish_and_clear();
                self.mp.remove(footer_pb);
            } else {
                footer_pb.tick();
            }
        }
    }

    fn add_pending_pb(&mut self, span_id: &span::Id) {
        let prev_val = self
            .pending_progress_bars
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.pending_spans.push_back(span_id.clone());

        // Show the footer progress bar.
        if let Some(footer_pb) = self.footer_pb.as_ref() {
            if prev_val == 0 {
                debug_assert!(
                    footer_pb.is_hidden(),
                    "footer progress bar was not hidden despite there being no pending progress bars"
                );

                footer_pb.reset();

                if let Some(tick_interval) = self.tick_settings.footer_tick_interval {
                    footer_pb.enable_steady_tick(tick_interval);
                }

                self.mp.add(footer_pb.clone());
                // Appears to have broken with
                // https://github.com/console-rs/indicatif/pull/648
                // self.mp.set_move_cursor(true);
            }

            footer_pb.tick();
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

            if let Some(tick_interval) = self.tick_settings.default_tick_interval {
                pb.enable_steady_tick(tick_interval);
            }

            pb.tick();

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
