//! Property-based robustness tests: program output is untrusted, so the
//! terminal must behave for *any* input, not only for the cases written by
//! hand (ADR 0012).
//!
//! Inputs are built from tokens that matter to the parser and the mark
//! scanner (escape sequences, marks, clears, control characters, high
//! bytes), mixed with plain text, so random cases reach deep states quickly.
//! When a property fails, proptest shrinks the input to a minimal example.

use proptest::prelude::*;
use woop_term::{
    BlockRows, BlockState, SelectionKind, SelectionPoint, Terminal, TerminalMode, TerminalSize,
    input,
};

/// Pieces of output that exercise the parser and the mark scanner, including
/// arbitrary bytes, so most inputs are invalid or split UTF-8.
fn token() -> impl Strategy<Value = Vec<u8>> {
    let fixed: Vec<&'static [u8]> = vec![
        b"\x1b]133;A\x07",
        b"\x1b]133;B\x07",
        b"\x1b]133;C\x07",
        b"\x1b]133;D;0\x07",
        b"\x1b]133;D;1\x1b\\",
        b"\x1b]0;title\x07",
        b"\x1b]",
        b"\x1b[",
        b"\x1b",
        b"\x1bc",
        b"\x1b[3J",
        b"\x1b[2J",
        b"\x1b[H",
        b"\x1b[?1049h",
        b"\x1b[?1049l",
        b"\x1b[?2026h",
        b"\x1b[?2026l",
        b"\x1b[5S",
        b"\x1b[3T",
        b"\x1b[2;4r",
        b"\x1b[1;31m",
        b"\x1b[0m",
        b"\x1b[6n",
        b"\x1bP",
        b"\x1b_",
        b"\r\n",
        b"\r",
        b"\n",
        b"\x07",
        b"\x18",
        b"\x1a",
        b"\x7f",
        b"\x08",
        b"\t",
        b"$ ",
        "日本".as_bytes(),
        "e\u{301}".as_bytes(),
    ];
    let symbols = prop::sample::select(vec![b';', b':', b'?', b'J', b'c', b'\\', b']', b'[']);
    // After ESC the parser ignores high bytes, so these pairs exercise the
    // scanner without producing invalid UTF-8 in text.
    let escaped_high =
        prop::sample::select(vec![0x7f_u8, 0x80, 0xc3, 0xff]).prop_map(|byte| vec![0x1b, byte]);
    let common = prop_oneof![
        4 => prop::sample::select(fixed).prop_map(<[u8]>::to_vec),
        3 => "[a-z ]{1,40}".prop_map(String::into_bytes),
        1 => (0u8..=9).prop_map(|digit| vec![b'0' + digit]),
        1 => symbols.prop_map(|byte| vec![byte]),
        1 => escaped_high,
        1 => prop::collection::vec(0u8..0x80, 1..8),
    ];
    prop_oneof![
        8 => common,
        1 => prop::collection::vec(any::<u8>(), 1..8),
        // Lead bytes of multi-byte characters, often left incomplete.
        1 => prop::sample::select(vec![0x80_u8, 0xc3, 0xcc, 0xce, 0xe6, 0xf0, 0xff]).prop_map(|byte| vec![byte]),
    ]
}

/// Arbitrary output, including invalid and split UTF-8.
fn output(max_tokens: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(token(), 0..max_tokens).prop_map(|tokens| tokens.concat())
}

/// Something that can happen to a terminal.
#[derive(Debug, Clone)]
enum Op {
    Output(Vec<u8>),
    Resize(u16, u16),
    Scroll(i32),
    Select(SelectionKind, usize, usize, usize, usize),
}

fn op() -> impl Strategy<Value = Op> {
    let kind = prop::sample::select(vec![
        SelectionKind::Simple,
        SelectionKind::Word,
        SelectionKind::Line,
        SelectionKind::Block,
    ]);
    prop_oneof![
        6 => output(60).prop_map(Op::Output),
        1 => (1u16..40, 1u16..120).prop_map(|(rows, cols)| Op::Resize(rows, cols)),
        1 => (-50i32..50).prop_map(Op::Scroll),
        1 => (kind, 0usize..60, 0usize..200, 0usize..60, 0usize..200)
            .prop_map(|(kind, r1, c1, r2, c2)| Op::Select(kind, r1, c1, r2, c2)),
    ]
}

/// Everything observable about a terminal, except timings.
#[derive(Debug, PartialEq)]
struct Observed {
    screen: woop_term::Screen,
    history: usize,
    blocks: Vec<(String, Option<i32>, BlockState, Option<BlockRows>)>,
}

fn observe(term: &Terminal) -> Observed {
    Observed {
        screen: term.screen(),
        history: term.history_size(),
        blocks: term
            .blocks()
            .iter()
            .map(|block| {
                (
                    block.command().to_owned(),
                    block.exit_code(),
                    block.state(),
                    term.block_rows(block),
                )
            })
            .collect(),
    }
}

/// Checks invariants that must hold after any input.
fn check_invariants(term: &Terminal) {
    let screen = term.screen();
    let size = term.size();
    assert_eq!(screen.rows, usize::from(size.rows));
    assert_eq!(screen.cols, usize::from(size.cols));
    assert_eq!(screen.cells.len(), screen.rows * screen.cols);
    if let Some(cursor) = screen.cursor {
        assert!(cursor.row < screen.rows && cursor.col < screen.cols);
    }
    let total_rows = term.history_size() + usize::from(size.rows);
    for block in term.blocks() {
        if let Some(rows) = term.block_rows(block) {
            assert!(rows.prompt < total_rows);
            for later in [rows.output, rows.end].into_iter().flatten() {
                assert!(later >= rows.prompt, "{rows:?}");
                assert!(later < total_rows);
            }
            if let (Some(output), Some(end)) = (rows.output, rows.end) {
                assert!(end >= output, "{rows:?}");
            }
        }
    }
    let _ = term.selection_text();
}

proptest! {
    // 256 cases per property by default; set PROPTEST_CASES to search
    // longer, for example `PROPTEST_CASES=20000 cargo test --release`.
    #![proptest_config(ProptestConfig::default())]

    /// No sequence of output, resizes, scrolling and selection panics or
    /// breaks the terminal's invariants.
    #[test]
    fn any_input_keeps_the_terminal_consistent(
        rows in 1u16..30,
        cols in 1u16..100,
        ops in prop::collection::vec(op(), 1..20),
    ) {
        let mut term = Terminal::new(TerminalSize::new(rows, cols));
        for op in ops {
            match op {
                Op::Output(bytes) => term.advance(&bytes),
                Op::Resize(rows, cols) => term.resize(TerminalSize::new(rows, cols)),
                Op::Scroll(lines) => term.scroll(lines),
                Op::Select(kind, r1, c1, r2, c2) => {
                    term.start_selection(kind, SelectionPoint::new(r1, c1));
                    term.extend_selection(SelectionPoint::new(r2, c2));
                }
            }
            check_invariants(&term);
            let _ = term.take_events();
        }
    }

    /// How output is split into reads never changes the result: the mark
    /// scanner and the emulator stay in step across read boundaries.
    #[test]
    fn splitting_output_into_reads_changes_nothing(
        bytes in output(200),
        cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..12),
    ) {
        let size = TerminalSize::new(8, 30);
        let mut whole = Terminal::new(size);
        whole.advance(&bytes);

        let mut split = Terminal::new(size);
        let mut points: Vec<usize> = cuts.iter().map(|cut| cut.index(bytes.len() + 1)).collect();
        points.sort_unstable();
        let mut start = 0;
        for point in points {
            split.advance(&bytes[start..point]);
            start = point;
        }
        split.advance(&bytes[start..]);

        prop_assert_eq!(observe(&whole), observe(&split));
    }

    /// Whatever state garbage leaves behind, a full reset brings the
    /// terminal back: nothing (an open sequence, a synchronized update, the
    /// alternate screen) can keep later output from being shown.
    #[test]
    fn the_terminal_always_recovers(garbage in output(200)) {
        let mut term = Terminal::new(TerminalSize::new(6, 30));
        term.advance(&garbage);
        term.advance(b"\x18\x1bc");
        term.advance(b"recovered");

        prop_assert!(term.screen_text().contains("recovered"), "{:?}", term.screen_text());
    }
}

/// Pasted text, biased towards control characters and line endings.
fn pasted() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            3 => "[a-z ]{1,10}",
            1 => prop::sample::select(vec!["\n", "\r\n", "\r", "\t", "\x1b", "\x1b[201~", "\x03", "\x7f", "\u{9b}", "\u{85}"])
                .prop_map(str::to_owned),
            1 => any::<char>().prop_map(String::from),
        ],
        0..30,
    )
    .prop_map(|parts| parts.concat())
}

fn modes() -> impl Strategy<Value = TerminalMode> {
    (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
        |(app_cursor, bracketed_paste, alt_screen)| TerminalMode {
            app_cursor,
            bracketed_paste,
            alt_screen,
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::default())]

    /// Pasted text is typed as text: it can never end a bracketed paste
    /// early or reach the shell as control keys.
    #[test]
    fn pasted_text_cannot_act_as_keys(text in pasted(), mode in modes()) {
        let bytes = input::encode_paste(&text, mode);
        let body = if mode.bracketed_paste {
            prop_assert!(bytes.starts_with(b"\x1b[200~"));
            prop_assert!(bytes.ends_with(b"\x1b[201~"));
            &bytes[6..bytes.len() - 6]
        } else {
            &bytes[..]
        };
        let body = std::str::from_utf8(body).expect("pasted text stays UTF-8");
        for ch in body.chars() {
            let allowed = !ch.is_control() || ch == '\t' || ch == '\r' || (mode.bracketed_paste && ch == '\n');
            prop_assert!(allowed, "{ch:?} in {body:?}");
        }
    }

    /// Every key, with every combination of modifiers and modes, encodes
    /// without panicking.
    #[test]
    fn every_key_encodes(
        key in prop_oneof![
            prop::sample::select(vec![
                input::Key::Enter, input::Key::Backspace, input::Key::Tab, input::Key::Escape,
                input::Key::Up, input::Key::Down, input::Key::Left, input::Key::Right,
                input::Key::Home, input::Key::End, input::Key::PageUp, input::Key::PageDown,
                input::Key::Insert, input::Key::Delete,
            ]),
            any::<u8>().prop_map(input::Key::F),
            any::<char>().prop_map(input::Key::Char),
        ],
        shift in any::<bool>(),
        ctrl in any::<bool>(),
        alt in any::<bool>(),
        mode in modes(),
    ) {
        let modifiers = input::Modifiers { shift, ctrl, alt };
        if let Some(bytes) = input::encode_key(key, modifiers, mode) {
            prop_assert!(!bytes.is_empty());
        }
    }
}
