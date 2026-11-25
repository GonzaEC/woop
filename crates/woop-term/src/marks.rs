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
/// marks and are skipped. Room for a prompt mark with a long, encoded
/// working directory (ADR 0022).
const MAX_PAYLOAD: usize = 4096;

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
    /// When set, OSC 133 marks count only if they carry this token as a
    /// `woop=<token>` option (ADR 0018).
    token: Option<Vec<u8>>,
    /// The working directory reported by the last accepted prompt mark,
    /// until taken (ADR 0022): `Some(None)` when that mark had no usable
    /// one.
    directory: Option<Option<String>>,
}

/// Shows only the scanner's state: the token, and the payload of a mark
/// being read, are secrets that must not end up in logs.
impl std::fmt::Debug for MarkScanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarkScanner")
            .field("state", &self.state)
            .field("requires_token", &self.token.is_some())
            .finish_non_exhaustive()
    }
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
            token: None,
            directory: None,
        }
    }
}

/// Whether the emulator stays in its escape state after `byte`: it executes
/// C0 controls other than CAN, SUB and ESC, and ignores DEL and bytes from
/// 0x80 up, waiting for the rest of the sequence.
fn stays_in_escape(byte: u8) -> bool {
    matches!(byte, 0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f..=0xff)
}

impl MarkScanner {
    /// From now on, accepts only marks that carry `token`.
    pub(crate) fn require_token(&mut self, token: &[u8]) {
        self.token = Some(token.to_vec());
    }

    /// The working directory reported by the last prompt mark accepted
    /// since the last call, if one was accepted: `Some(None)` when that mark
    /// had no usable directory, which then is not known any more.
    pub(crate) fn take_directory(&mut self) -> Option<Option<String>> {
        self.directory.take()
    }

    /// Parses a complete OSC payload into a mark, checking the token.
    fn mark(&self, payload: &[u8]) -> Option<Mark> {
        parse(payload, self.token.as_deref())
    }

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
                // C0 controls, DEL and high bytes do not end an escape
                // sequence; the emulator keeps waiting for the rest.
                (State::Escape | State::OscEscape | State::SkipEscape, ESC) => State::Escape,
                (State::Escape | State::OscEscape | State::SkipEscape, _)
                    if stays_in_escape(byte) =>
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
                    if let Some(mark) = self.mark(&self.payload) {
                        if mark == Mark::PromptStart {
                            self.directory = Some(directory(&self.payload));
                        }
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
                    if let Some(mark) = self.mark(&self.payload) {
                        if mark == Mark::PromptStart {
                            self.directory = Some(directory(&self.payload));
                        }
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

/// Parses an OSC payload such as `133;D;0;woop=<token>`. With a `token`,
/// the payload must carry it in a `woop=` option.
fn parse(payload: &[u8], token: Option<&[u8]>) -> Option<Mark> {
    let rest = payload.strip_prefix(b"133;")?;
    if let Some(token) = token {
        let carried = rest
            .split(|&byte| byte == b';')
            .skip(1)
            .any(|field| field.strip_prefix(b"woop=") == Some(token));
        if !carried {
            return None;
        }
    }
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

/// The working directory in an accepted prompt mark's `cwd=` option
/// (ADR 0022): percent-decoded, valid UTF-8 without control characters,
/// and an absolute path: one of this platform, or a POSIX one (bash on
/// Windows reports `/c/Users`, which completion converts, ADR 0023).
/// Anything else is ignored.
fn directory(payload: &[u8]) -> Option<String> {
    let encoded = payload
        .split(|&byte| byte == b';')
        .skip(2)
        .find_map(|field| field.strip_prefix(b"cwd="))?;
    let directory = String::from_utf8(percent_decode(encoded)?).ok()?;
    let valid = !directory.is_empty()
        && !directory.chars().any(char::is_control)
        && (std::path::Path::new(&directory).is_absolute() || directory.starts_with('/'));
    valid.then_some(directory)
}

/// Decodes `%XX` escapes; any other `%` makes the text invalid.
fn percent_decode(text: &[u8]) -> Option<Vec<u8>> {
    let hex = |byte: u8| char::from(byte).to_digit(16);
    let mut decoded = Vec::with_capacity(text.len());
    let mut bytes = text.iter();
    while let Some(&byte) = bytes.next() {
        if byte == b'%' {
            let high = hex(*bytes.next()?)?;
            let low = hex(*bytes.next()?)?;
            decoded.push(u8::try_from(high * 16 + low).ok()?);
        } else {
            decoded.push(byte);
        }
    }
    Some(decoded)
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
        assert_eq!(
            parse(b"133;D;0", None),
            Some(Mark::CommandFinished(Some(0)))
        );
        assert_eq!(
            parse(b"133;D;-1", None),
            Some(Mark::CommandFinished(Some(-1)))
        );
        assert_eq!(parse(b"133;D", None), Some(Mark::CommandFinished(None)));
        assert_eq!(parse(b"133;D;x", None), Some(Mark::CommandFinished(None)));
    }

    #[test]
    fn accepts_extra_options() {
        assert_eq!(parse(b"133;A;cl=m;aid=1", None), Some(Mark::PromptStart));
    }

    #[test]
    fn with_a_token_only_marks_carrying_it_count() {
        let token = Some(&b"abc123"[..]);
        assert_eq!(parse(b"133;A;woop=abc123", token), Some(Mark::PromptStart));
        assert_eq!(
            parse(b"133;D;2;woop=abc123", token),
            Some(Mark::CommandFinished(Some(2)))
        );
        assert_eq!(
            parse(b"133;B;aid=1;woop=abc123", token),
            Some(Mark::CommandStart)
        );
        // Forged marks: no token, a wrong one, a prefix or an extension of
        // it, or the token in place of the mark letter.
        for forged in [
            &b"133;A"[..],
            b"133;A;woop=",
            b"133;A;woop=abc12",
            b"133;A;woop=abc1234",
            b"133;A;woop=ABC123",
            b"133;A;xwoop=abc123",
            b"133;woop=abc123",
            b"133;D;woop=abc12;0",
        ] {
            assert_eq!(parse(forged, token), None, "{forged:?}");
        }
    }

    /// Percent-encodes as the integration scripts do.
    fn encode(text: &str) -> String {
        text.bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"-._~/:\\".contains(&byte) {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect()
    }

    /// An absolute path on this platform, with characters that need encoding.
    fn absolute(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn signed_prompt_marks_report_the_directory() {
        let directory = absolute("my dir;with%odd ñ");
        let mut scanner = MarkScanner::default();
        scanner.require_token(b"t0k");
        let mark = format!("\x1b]133;A;woop=t0k;cwd={}\x07", encode(&directory));
        assert_eq!(scanner.scan(mark.as_bytes()).len(), 1);
        assert_eq!(scanner.take_directory(), Some(Some(directory.clone())));
        assert_eq!(scanner.take_directory(), None, "taken once");

        // A forged mark, without the token, sets nothing.
        let forged = format!("\x1b]133;A;cwd={}\x07", encode(&absolute("evil")));
        scanner.scan(forged.as_bytes());
        assert_eq!(scanner.take_directory(), None);
    }

    #[test]
    fn a_signed_prompt_without_a_directory_reports_none() {
        let mut scanner = MarkScanner::default();
        scanner.require_token(b"t0k");
        let first = format!("\x1b]133;A;woop=t0k;cwd={}\x07", encode(&absolute("a")));
        scanner.scan(first.as_bytes());
        // A later prompt with no usable directory replaces it, in the same
        // read or a later one.
        scanner.scan(b"\x1b]133;A;woop=t0k\x07");
        assert_eq!(scanner.take_directory(), Some(None));
        scanner.scan(b"\x1b]133;A;woop=t0k;cwd=%FF\x1b\\");
        assert_eq!(scanner.take_directory(), Some(None));
    }

    #[test]
    fn invalid_directories_are_ignored() {
        let check = |encoded: &str| {
            let mut scanner = MarkScanner::default();
            scanner.scan(format!("\x1b]133;A;cwd={encoded}\x07").as_bytes());
            scanner.take_directory()
        };
        // Relative, empty, broken escapes, control characters, not UTF-8.
        for encoded in ["relative/dir", "", "%zz", "%4", "%0A", "%FF%FE"] {
            assert_eq!(check(encoded), Some(None), "{encoded:?}");
        }
        let with_newline = format!("{}%0A", encode(&absolute("x")));
        assert_eq!(check(&with_newline), Some(None));
        // Only prompt-start marks report a directory.
        let mut scanner = MarkScanner::default();
        scanner.scan(format!("\x1b]133;B;cwd={}\x07", encode(&absolute("x"))).as_bytes());
        assert_eq!(scanner.take_directory(), None);
        // POSIX paths are kept on every platform: bash on Windows reports
        // its directory as `/c/Users`.
        assert_eq!(check("/c/Users/me"), Some(Some("/c/Users/me".to_owned())));
    }

    #[test]
    fn long_directories_fit_up_to_the_payload_limit() {
        let directory = absolute(&"d".repeat(MAX_PAYLOAD - 200));
        let mut scanner = MarkScanner::default();
        scanner.scan(format!("\x1b]133;A;cwd={}\x07", encode(&directory)).as_bytes());
        assert_eq!(scanner.take_directory(), Some(Some(directory)));
    }

    #[test]
    fn the_scanner_requires_the_token_once_set() {
        let mut scanner = MarkScanner::default();
        scanner.require_token(b"t0k");
        let bytes = b"\x1b]133;A\x07\x1b]133;A;woop=t0k\x07\x1b[3J";
        assert_eq!(
            scanner.scan(bytes),
            [
                (bytes.len() - 4, Mark::PromptStart),
                // History clears are not marks and need no token.
                (bytes.len(), Mark::HistoryCleared)
            ]
        );
    }

    #[test]
    fn ignores_other_sequences() {
        assert_eq!(parse(b"0;title", None), None);
        assert_eq!(parse(b"1337;A", None), None);
        assert_eq!(parse(b"133;Z", None), None);
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
