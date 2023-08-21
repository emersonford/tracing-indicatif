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

use crate::span_ext::IndicatifSpanExt;
use crate::suspend_tracing_indicatif;
use crate::IndicatifLayer;

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
}

impl Default for HelpersConfig {
    fn default() -> Self {
        Self { show_footer: true }
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
        .with_span_child_prefix_symbol("> ");

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
...and 1 more not shown above.
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
...and 2 more not shown above.
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
...and 1 more not shown above.
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
    let (subscriber, term) = make_helpers(HelpersConfig::default());

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
