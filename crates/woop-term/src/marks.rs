//! Detection of shell integration marks (OSC 133) and of history clears in
//! the output stream.
//!
//! The emulator ignores the marks and gives no notice when the scrollback is
//! cleared, so Woop looks for both itself before the bytes reach the parser.
//! The scanner keeps its state between calls, so a sequence split across two
//! reads is still found.

/// A sequence in the output that block tracking must react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    /// `ESC c` (full reset) or `CSI 3 J` (erase the scrollback): positions
    /// in the history are no longer valid.
    HistoryCleared,
    /// An OSC sequence reached [`MAX_OSC_BYTES`] without ending. The
    /// emulator buffers OSC payloads without limit, so the terminal cancels
    /// the sequence at this point.
    SequenceTooLong,
    /// `OSC 133 ; A`: the prompt is about to be printed.
    PromptStart,
    /// `OSC 133 ; B`: the prompt ended; the user types the command next.
    CommandStart,
    /// `OSC 133 ; C`: the command was submitted; its output follows.
    OutputStart,
    /// `OSC 133 ; D [; code]`: the command finished, with its exit code
    /// when the shell reports one.
    CommandFinished(Option<i32>),
}

/// Longest OSC payload the scanner buffers. Longer sequences cannot be
/// marks and are skipped.
const MAX_PAYLOAD: usize = 256;

/// Longest OSC sequence allowed to reach the emulator. Legitimate ones
/// (titles, links, marks) are far shorter.
pub(crate) const MAX_OSC_BYTES: usize = 64 * 1024;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// CAN and SUB cancel an escape sequence.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Outside any escape sequence.
    Ground,
    /// After `ESC`.
    Escape,
    /// Inside `ESC ]`, collecting the payload.
    Osc,
    /// Inside an OSC, after an `ESC` that may start the `ESC \` terminator.
    OscEscape,
    /// Inside an OSC too long to be a mark, waiting for its end.
    Skip,
    /// Inside a skipped OSC, after an `ESC`.
    SkipEscape,
    /// Inside `ESC [`, collecting parameters until the final byte.
    Csi,
}

/// Incremental scanner for OSC 133 marks.
#[derive(Debug)]
pub(crate) struct MarkScanner {
    state: State,
    payload: Vec<u8>,
    /// Length of the OSC being skipped so far.
    osc_len: usize,
    /// Value of the first CSI parameter, as the emulator computes it.
    csi_first: u16,
    /// The first CSI parameter has ended (`;` or `:` was seen).
    csi_after_first: bool,
    /// The CSI has a private marker or an intermediate byte, so it is not a
    /// plain erase command.
    csi_other: bool,
}

impl Default for MarkScanner {
    fn default() -> Self {
        Self {
            state: State::Ground,
            payload: Vec::with_capacity(MAX_PAYLOAD),
            osc_len: 0,
            csi_first: 0,
            csi_after_first: false,
            csi_other: false,
        }
    }
}

/// Whether the emulator executes `byte` inside an escape sequence without
/// leaving it: C0 controls other than CAN, SUB and ESC.
fn executes_in_escape(byte: u8) -> bool {
    matches!(byte, 0x00..=0x17 | 0x19 | 0x1c..=0x1f)
}

impl MarkScanner {
    /// Scans `bytes` and returns each mark found, with the index just after
    /// its terminator. Feeding the emulator up to that index and then
    /// handling the mark keeps marks in step with the text around them.
    pub(crate) fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, Mark)> {
        let mut marks = Vec::new();
        for (index, &byte) in bytes.iter().enumerate() {
            self.state = match (self.state, byte) {
                (State::Ground, ESC) => State::Escape,
                (State::Ground, _) => State::Ground,
                // An ESC inside an OSC also ends it and starts a new
                // sequence, except for the `ESC \` terminator handled below.
                (State::Escape | State::OscEscape | State::SkipEscape, b']') => {
                    self.payload.clear();
                    State::Osc
                }
                (State::Escape | State::OscEscape | State::SkipEscape, b'[') => {
                    self.csi_first = 0;
                    self.csi_after_first = false;
                    self.csi_other = false;
                    State::Csi
                }
                (State::Escape | State::OscEscape | State::SkipEscape, b'c') => {
                    marks.push((index + 1, Mark::HistoryCleared));
                    State::Ground
                }
                // C0 controls are executed inside an escape sequence without
                // ending it; the emulator keeps waiting for the rest.
                (State::Escape | State::OscEscape | State::SkipEscape, ESC) => State::Escape,
                (State::Escape | State::OscEscape | State::SkipEscape, _)
                    if executes_in_escape(byte) =>
                {
                    State::Escape
                }
                (State::Escape, _) => State::Ground,
                (State::Csi, CAN | SUB) => State::Ground,
                (State::Csi, ESC) => State::Escape,
                (State::Csi, 0x40..=0x7e) => {
                    // Only the first parameter counts, as a number: `3J`,
                    // `03J`, `3;J` and `3:1J` all erase the scrollback.
                    if byte == b'J' && !self.csi_other && self.csi_first == 3 {
                        marks.push((index + 1, Mark::HistoryCleared));
                    }
                    State::Ground
                }
                (State::Csi, b'0'..=b'9') => {
                    if !self.csi_after_first {
                        self.csi_first = self
                            .csi_first
                            .saturating_mul(10)
                            .saturating_add(u16::from(byte - b'0'));
                    }
                    State::Csi
                }
                (State::Csi, b';' | b':') => {
                    self.csi_after_first = true;
                    State::Csi
                }
                // Private markers (`<`, `=`, `>`, `?`) and intermediates.
                (State::Csi, 0x20..=0x3f) => {
                    self.csi_other = true;
                    State::Csi
                }
                // Other control characters are executed without ending the
                // sequence.
                (State::Csi, _) => State::Csi,
                (State::Osc, BEL) => {
                    if let Some(mark) = parse(&self.payload) {
                        marks.push((index + 1, mark));
                    }
                    State::Ground
                }
                (State::Osc | State::Skip, CAN | SUB) => State::Ground,
                (State::Osc, ESC) => State::OscEscape,
                (State::Osc, _) if self.payload.len() >= MAX_PAYLOAD => {
                    self.osc_len = self.payload.len() + 1;
                    State::Skip
                }
                (State::Osc, _) => {
                    self.payload.push(byte);
                    State::Osc
                }
                (State::OscEscape, b'\\') => {
                    if let Some(mark) = parse(&self.payload) {
                        marks.push((index + 1, mark));
                    }
                    State::Ground
                }
                (State::OscEscape, _) => State::Ground,
                (State::Skip, BEL) => State::Ground,
                (State::Skip, ESC) => State::SkipEscape,
                (State::Skip, _) => {
                    self.osc_len += 1;
                    if self.osc_len >= MAX_OSC_BYTES {
                        marks.push((index + 1, Mark::SequenceTooLong));
                        State::Ground
                    } else {
                        State::Skip
                    }
                }
                (State::SkipEscape, _) => State::Ground,
            };
        }
        marks
    }
}

/// Parses an OSC payload such as `133;D;0`.
fn parse(payload: &[u8]) -> Option<Mark> {
    let rest = payload.strip_prefix(b"133;")?;
    let mut fields = rest.split(|&byte| byte == b';');
    let mark = match fields.next()? {
        b"A" => Mark::PromptStart,
        b"B" => Mark::CommandStart,
        b"C" => Mark::OutputStart,
        b"D" => Mark::CommandFinished(
            fields
                .next()
                .and_then(|code| std::str::from_utf8(code).ok())
                .and_then(|code| code.parse().ok()),
        ),
        _ => return None,
    };
    Some(mark)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<Vec<(usize, Mark)>> {
        let mut scanner = MarkScanner::default();
        chunks.iter().map(|chunk| scanner.scan(chunk)).collect()
    }

    #[test]
    fn finds_marks_with_either_terminator() {
        let bytes = b"\x1b]133;A\x07$ \x1b]133;B\x1b\\ls\r\n\x1b]133;C\x07";
        assert_eq!(
            scan(&[bytes]),
            [vec![
                (8, Mark::PromptStart),
                (19, Mark::CommandStart),
                (bytes.len(), Mark::OutputStart)
            ]]
        );
    }

    #[test]
    fn reads_exit_codes() {
        assert_eq!(parse(b"133;D;0"), Some(Mark::CommandFinished(Some(0))));
        assert_eq!(parse(b"133;D;-1"), Some(Mark::CommandFinished(Some(-1))));
        assert_eq!(parse(b"133;D"), Some(Mark::CommandFinished(None)));
        assert_eq!(parse(b"133;D;x"), Some(Mark::CommandFinished(None)));
    }

    #[test]
    fn accepts_extra_options() {
        assert_eq!(parse(b"133;A;cl=m;aid=1"), Some(Mark::PromptStart));
    }

    #[test]
    fn ignores_other_sequences() {
        assert_eq!(parse(b"0;title"), None);
        assert_eq!(parse(b"1337;A"), None);
        assert_eq!(parse(b"133;Z"), None);
        assert_eq!(scan(&[b"\x1b[31mred\x1b]0;title\x07"]), [vec![]]);
    }

    #[test]
    fn finds_marks_split_across_reads() {
        assert_eq!(
            scan(&[b"text\x1b]13", b"3;D;", b"2\x1b", b"\\more"]),
            [
                vec![],
                vec![],
                vec![],
                vec![(1, Mark::CommandFinished(Some(2)))]
            ]
        );
    }

    #[test]
    fn finds_history_clears() {
        let clear = Mark::HistoryCleared;
        assert_eq!(scan(&[b"a\x1b[3Jb"]), [vec![(5, clear)]]);
        assert_eq!(scan(&[b"\x1bc"]), [vec![(2, clear)]]);
        // Split across reads, and right after an aborted OSC.
        assert_eq!(
            scan(&[b"\x1b[", b"3", b"J"]),
            [vec![], vec![], vec![(1, clear)]]
        );
        assert_eq!(scan(&[b"\x1b]0;t\x1bc"]), [vec![(7, clear)]]);
    }

    #[test]
    fn cuts_off_endless_sequences() {
        let mut endless = b"\x1b]0;".to_vec();
        endless.extend(std::iter::repeat_n(b'x', MAX_OSC_BYTES + 10));
        let found = scan(&[&endless]);
        assert_eq!(found[0].len(), 1);
        assert_eq!(found[0][0].1, Mark::SequenceTooLong);
        assert!(found[0][0].0 <= MAX_OSC_BYTES + 4);

        // CAN cancels an OSC, so the mark after it is found.
        assert_eq!(
            scan(&[b"\x1b]133;A\x18\x1b]133;B\x07"]),
            [vec![(16, Mark::CommandStart)]]
        );
    }

    #[test]
    fn other_erase_sequences_are_not_history_clears() {
        // Erasing the screen keeps the scrollback.
        assert_eq!(
            scan(&[b"\x1b[2J\x1b[J\x1b[13J\x1b[?3J\x1b[3$J\x1b[;3J\x1b[3K"]),
            [vec![]]
        );
    }

    #[test]
    fn history_clears_are_read_like_the_emulator_reads_them() {
        // The first parameter is a number, and control characters inside a
        // sequence do not end it.
        let zeros = format!("\x1b[{}3J", "0".repeat(40));
        for sequence in [
            zeros.as_bytes(),
            b"\x1b[3;J",
            b"\x1b[3:1J",
            b"\x1b\x00c",
            b"\x1b\r[3J",
            b"\x1b[\r3J",
        ] {
            assert_eq!(
                scan(&[sequence]),
                [vec![(sequence.len(), Mark::HistoryCleared)]],
                "{sequence:?}"
            );
        }
    }

    #[test]
    fn skips_long_sequences() {
        let mut long = b"\x1b]133;A".to_vec();
        long.extend(std::iter::repeat_n(b'x', MAX_PAYLOAD));
        long.extend(b"\x07\x1b]133;B\x07");
        assert_eq!(scan(&[&long]), [vec![(long.len(), Mark::CommandStart)]]);
    }
}
