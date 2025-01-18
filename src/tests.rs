use std::io;
use std::thread;
use std::time::Duration;

use indicatif::InMemoryTerm;
use indicatif::MultiProgress;
use indicatif::ProgressDrawTarget;
use indicatif::ProgressStyle;
use indicatif::TermLike;
use tracing::info;
use tracing::info_span;
use tracing_core::Subscriber;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

use crate::filter::hide_indicatif_span_fields;
use crate::span_ext::IndicatifSpanExt;
use crate::suspend_tracing_indicatif;
use crate::IndicatifLayer;
use crate::TickSettings;

#[derive(Clone)]
struct InMemoryTermWriter {
    progress_bars: Option<MultiProgress>,
    term: InMemoryTerm,
}

impl io::Write for InMemoryTermWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(ref pb) = self.progress_bars {
            pb.suspend(|| self.term.write_str(std::str::from_utf8(buf).unwrap()))?;
        } else {
            self.term.write_str(std::str::from_utf8(buf).unwrap())?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(ref pb) = self.progress_bars {
            pb.suspend(|| self.term.flush())
        } else {
            self.term.flush()
        }
    }
}

impl<'a> MakeWriter<'a> for InMemoryTermWriter {
    type Writer = InMemoryTermWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

struct HelpersConfig {
    show_footer: bool,
    enable_steady_tick: bool,
}

impl Default for HelpersConfig {
    fn default() -> Self {
        Self {
            show_footer: true,
            enable_steady_tick: false,
        }
    }
}

fn make_helpers(config: HelpersConfig) -> (impl Subscriber, InMemoryTerm) {
    let indicatif_layer = IndicatifLayer::new()
        .with_max_progress_bars(
            5,
            config.show_footer.then(|| {
                ProgressStyle::with_template("...and {pending_progress_bars} more not shown above.")
                    .unwrap()
            }),
        )
        .with_span_field_formatter(DefaultFields::new())
        .with_progress_style(
            ProgressStyle::with_template("{span_child_prefix}{span_name}{{{span_fields}}}")
                .unwrap(),
        )
        .with_span_child_prefix_indent("--")
        .with_span_child_prefix_symbol("> ")
        .with_tick_settings(TickSettings {
            term_draw_hz: 20,
            default_tick_interval: if config.enable_steady_tick {
                Some(Duration::from_millis(50))
            } else {
                None
            },
            footer_tick_interval: None,
            ..Default::default()
        });

    let term = InMemoryTerm::new(10, 100);

    let mp = indicatif_layer.pb_manager.lock().unwrap().mp.clone();

    mp.set_draw_target(ProgressDrawTarget::term_like(Box::new(term.clone())));

    let writer = InMemoryTermWriter {
        progress_bars: Some(mp),
        term: term.clone(),
    };

    (
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .without_time()
                    .with_writer(writer),
            )
            .with(indicatif_layer),
        term,
    )
}

#[test]
fn test_one_basic_pb() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let _span = info_span!("foo").entered();
        thread::sleep(Duration::from_millis(10));

        assert_eq!(
            term.contents(),
            r#"
foo{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_one_child_pb() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let _span = info_span!("foo").entered();
        let _child_span = info_span!("child").entered();
        thread::sleep(Duration::from_millis(10));

        assert_eq!(
            term.contents(),
            r#"
foo{}
--> child{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_span_fields() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let _span = info_span!("foo", val = 3).entered();

        thread::sleep(Duration::from_millis(10));

        assert_eq!(
            term.contents(),
            r#"
foo{val=3}
            "#
            .trim()
        );
    });
}

#[test]
fn test_multi_child_pb() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let _span1 = info_span!("foo", blah = 1);
        let _span1_enter = _span1.enter();

        let _child_span1 = info_span!("foo.child");
        let _child_span1_enter = _child_span1.enter();

        let _child_child_span1 = info_span!("foo.child.child", blah = 3, hello = "world");
        let _child_child_span1_enter = _child_child_span1.enter();

        std::mem::drop(_span1_enter);
        std::mem::drop(_child_span1_enter);
        std::mem::drop(_child_child_span1_enter);

        let _span2 = info_span!("bar");
        let _span2_enter = _span2.enter();

        let _child_span2 = info_span!("bar.child");
        let _child_span2_enter = _child_span2.enter();

        std::mem::drop(_span2_enter);
        std::mem::drop(_child_span2_enter);

        thread::sleep(Duration::from_millis(10));

        assert_eq!(
            term.contents(),
            r#"
foo{blah=1}
--> foo.child{}
----> foo.child.child{blah=3 hello="world"}
bar{}
--> bar.child{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_max_pbs() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let _span1 = info_span!("1");
        _span1.pb_start();
        let _span2 = info_span!("2");
        _span2.pb_start();
        let _span3 = info_span!("3");
        _span3.pb_start();
        let _span4 = info_span!("4");
        _span4.pb_start();
        let _span5 = info_span!("5");
        _span5.pb_start();

        assert_eq!(
            term.contents(),
            r#"
1{}
2{}
3{}
4{}
5{}
            "#
            .trim()
        );

        let _span6 = info_span!("6");
        _span6.pb_start();

        assert_eq!(
            term.contents(),
            r#"
1{}
2{}
3{}
4{}
5{}
...and 1 more not shown above.
            "#
            .trim()
        );

        let _span7 = info_span!("7");
        _span7.pb_start();

        assert_eq!(
            term.contents(),
            r#"
1{}
2{}
3{}
4{}
5{}
...and 2 more not shown above.
            "#
            .trim()
        );

        std::mem::drop(_span6);

        assert_eq!(
            term.contents(),
            r#"
1{}
2{}
3{}
4{}
5{}
...and 1 more not shown above.
            "#
            .trim()
        );

        std::mem::drop(_span1);

        assert_eq!(
            term.contents(),
            r#"
2{}
3{}
4{}
5{}
7{}
            "#
            .trim()
        );

        std::mem::drop(_span2);

        assert_eq!(
            term.contents(),
            r#"
3{}
4{}
5{}
7{}
            "#
            .trim()
        );

        let _span8 = info_span!("8");
        _span8.pb_start();

        assert_eq!(
            term.contents(),
            r#"
3{}
4{}
5{}
7{}
8{}
            "#
            .trim()
        );

        let _span9 = info_span!("9");
        _span9.pb_start();

        assert_eq!(
            term.contents(),
            r#"
3{}
4{}
5{}
7{}
8{}
...and 1 more not shown above.
            "#
            .trim()
        );

        let _span10 = info_span!("10");
        _span10.pb_start();

        assert_eq!(
            term.contents(),
            r#"
3{}
4{}
5{}
7{}
8{}
...and 2 more not shown above.
            "#
            .trim()
        );

        drop(_span3);

        assert_eq!(
            term.contents(),
            r#"
4{}
5{}
7{}
8{}
9{}
...and 1 more not shown above.
            "#
            .trim()
        );

        drop(_span4);

        assert_eq!(
            term.contents(),
            r#"
5{}
7{}
8{}
9{}
10{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_max_pbs_no_footer() {
    let (subscriber, term) = make_helpers(HelpersConfig {
        show_footer: false,
        ..Default::default()
    });

    tracing::subscriber::with_default(subscriber, || {
        let _span1 = info_span!("1");
        _span1.pb_start();
        let _span2 = info_span!("2");
        _span2.pb_start();
        let _span3 = info_span!("3");
        _span3.pb_start();
        let _span4 = info_span!("4");
        _span4.pb_start();
        let _span5 = info_span!("5");
        _span5.pb_start();
        let _span6 = info_span!("6");
        _span6.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
1{}
2{}
3{}
4{}
5{}
            "#
            .trim()
        );

        let _span7 = info_span!("7");
        _span7.pb_start();

        // Need 150ms of sleep here to trigger a refresh of the footer.
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
1{}
2{}
3{}
4{}
5{}
            "#
            .trim()
        );

        std::mem::drop(_span6);

        // Need 150ms of sleep here to trigger a refresh of the footer.
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
1{}
2{}
3{}
4{}
5{}
            "#
            .trim()
        );

        std::mem::drop(_span1);

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
2{}
3{}
4{}
5{}
7{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_parent_no_enter_doesnt_panic() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let span = info_span!("foo");
        let _child_span = info_span!(parent: &span, "child").entered();
        thread::sleep(Duration::from_millis(10));

        assert_eq!(
            term.contents(),
            r#"
foo{}
--> child{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_log_statements_coexist() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let _span1 = info_span!("foo");
        _span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
foo{}
            "#
            .trim()
        );

        info!("hello world!");
        assert_eq!(
            term.contents()
                .lines()
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join("\n"),
            r#"
INFO tracing_indicatif::tests: hello world!
foo{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_change_style_before_show() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("foo");
        span1.pb_set_style(&ProgressStyle::with_template("hello_world").unwrap());
        span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
hello_world
            "#
            .trim()
        );
    });
}

#[test]
fn test_change_style_after_show() {
    let (subscriber, term) = make_helpers(HelpersConfig {
        enable_steady_tick: true,
        ..Default::default()
    });

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("foo");
        span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
foo{}
            "#
            .trim()
        );

        span1.pb_set_style(&ProgressStyle::with_template("hello_world").unwrap());
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
hello_world
            "#
            .trim()
        );
    });
}

#[test]
fn test_change_style_after_show_tick() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("foo");
        span1.pb_start();

        assert_eq!(
            term.contents(),
            r#"
foo{}
            "#
            .trim()
        );

        span1.pb_set_style(&ProgressStyle::with_template("hello_world").unwrap());
        span1.pb_tick();
        assert_eq!(
            term.contents(),
            r#"
hello_world
            "#
            .trim()
        );
    });
}

#[test]
fn test_bar_style_progress_bar() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("foo");
        span1.pb_set_style(&ProgressStyle::default_bar());
        span1.pb_set_length(10);
        span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 0/10
            "#
            .trim()
        );

        span1.pb_inc(1);

        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
█████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 1/10
            "#
            .trim()
        );

        span1.pb_inc(9);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
██████████████████████████████████████████████████████████████████████████████████████████████ 10/10
            "#
            .trim()
        );
    });
}

#[test]
fn test_bar_style_progress_bar_inc_before_start() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("foo");
        span1.pb_set_style(&ProgressStyle::default_bar());
        span1.pb_set_length(10);
        span1.pb_inc_length(1);
        span1.pb_inc(2);
        span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
█████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 2/11
            "#
            .trim()
        );

        span1.pb_inc(1);

        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
█████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 3/11
            "#
            .trim()
        );

        span1.pb_inc_length(1);
        span1.pb_inc(9);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
██████████████████████████████████████████████████████████████████████████████████████████████ 12/12
            "#
            .trim()
        );
    });
}

#[test]
fn test_bar_style_progress_bar_inc_without_set_length() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("foo");
        span1.pb_set_style(&ProgressStyle::default_bar());
        span1.pb_inc_length(5);
        span1.pb_inc(2);
        span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 2/2
            "#
            .trim()
        );

        span1.pb_inc_length(5);
        span1.pb_inc(1);

        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 3/3
            "#
            .trim()
        );

        span1.pb_inc(8);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            term.contents(),
            r#"
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 11/11
            "#
            .trim()
        );
    });
}

#[test]
fn test_span_ext_no_effect_when_layer_not_added() {
    let term = InMemoryTerm::new(10, 100);

    let writer = InMemoryTermWriter {
        progress_bars: None,
        term: term.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .without_time()
            .with_writer(writer),
    );

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("foo");
        span1.pb_set_style(&ProgressStyle::default_bar());
        span1.pb_set_length(10);
        span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        info!("hello world!");
        thread::sleep(Duration::from_millis(10));

        assert_eq!(
            term.contents().trim(),
            r#"
INFO tracing_indicatif::tests: hello world!
            "#
            .trim()
        );
    });
}

#[test]
fn test_suspend_with_layer() {
    let (subscriber, term) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let _span1 = info_span!("foo");
        _span1.pb_start();

        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            term.contents(),
            r#"
foo{}
            "#
            .trim()
        );

        let _ = suspend_tracing_indicatif(|| term.write_line("hello world"));

        assert_eq!(
            term.contents()
                .lines()
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join("\n"),
            r#"
hello world
foo{}
            "#
            .trim()
        );

        let _ = suspend_tracing_indicatif(|| term.write_line("this is a test"));

        assert_eq!(
            term.contents()
                .lines()
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join("\n"),
            r#"
hello world
this is a test
foo{}
            "#
            .trim()
        );
    });
}

#[test]
fn test_suspend_without_layer() {
    let term = InMemoryTerm::new(10, 100);

    assert_eq!(
        term.contents(),
        r#"
        "#
        .trim()
    );

    let _ = suspend_tracing_indicatif(|| term.write_line("hello world"));

    assert_eq!(
        term.contents()
            .lines()
            .map(|line| line.trim())
            .collect::<Vec<_>>()
            .join("\n"),
        r#"
hello world
        "#
        .trim()
    );
}

#[test]
fn test_with_span_field_formatter() {
    let indicatif_layer = IndicatifLayer::new()
        .with_span_field_formatter(hide_indicatif_span_fields(DefaultFields::new()));

    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer);

    tracing::subscriber::with_default(subscriber, || {
        let _span = info_span!("foo");
        _span.pb_start();

        suspend_tracing_indicatif(|| {});
    });
}

#[test]
fn test_parent_span_enter_ordering() {
    let (subscriber, _) = make_helpers(HelpersConfig::default());

    tracing::subscriber::with_default(subscriber, || {
        let grandparent_span = info_span!("grandparent");
        let parent_span = info_span!(parent: &grandparent_span, "parent");
        let child_span = info_span!(parent: &parent_span, "child");

        let span1 = info_span!("span1");
        span1.pb_start();
        let span2 = info_span!("span2");
        span2.pb_start();
        let span3 = info_span!("span3");
        span3.pb_start();
        let span4 = info_span!("span4");
        span4.pb_start();
        let span5 = info_span!("span5");
        span5.pb_start();

        child_span.pb_start();
        grandparent_span.pb_start();

        drop(span1);
    });
}

// These don't actually run anything, but exist to type check macros.
#[allow(dead_code)]
fn type_check_indicatif_println() {
    indicatif_println!("{}", "hello");
}

#[allow(dead_code)]
fn type_check_indicatif_eprintln() {
    indicatif_eprintln!("{}", "hello");
}
