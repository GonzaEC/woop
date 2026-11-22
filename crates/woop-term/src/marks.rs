//! Detection of shell integration marks (OSC 133) in the output stream.
//!
//! The emulator ignores these sequences, so Woop looks for them itself
//! before the bytes reach the parser. The scanner keeps its state between
//! calls, so a mark split across two reads is still found.

/// A semantic mark emitted by a shell with Woop's integration loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
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

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

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
}

/// Incremental scanner for OSC 133 marks.
#[derive(Debug)]
pub(crate) struct MarkScanner {
    state: State,
    payload: Vec<u8>,
}

impl Default for MarkScanner {
    fn default() -> Self {
        Self {
            state: State::Ground,
            payload: Vec::with_capacity(MAX_PAYLOAD),
        }
    }
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
                (State::Escape, b']') => {
                    self.payload.clear();
                    State::Osc
                }
                (State::Escape, ESC) => State::Escape,
                (State::Escape, _) => State::Ground,
                (State::Osc, BEL) => {
                    if let Some(mark) = parse(&self.payload) {
                        marks.push((index + 1, mark));
                    }
                    State::Ground
                }
                (State::Osc, ESC) => State::OscEscape,
                (State::Osc, _) if self.payload.len() >= MAX_PAYLOAD => State::Skip,
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
                // Any other byte after ESC aborts the OSC and may start a
                // new sequence.
                (State::OscEscape, b']') => {
                    self.payload.clear();
                    State::Osc
                }
                (State::OscEscape, _) => State::Ground,
                (State::Skip, BEL) => State::Ground,
                (State::Skip, ESC) => State::SkipEscape,
                (State::Skip, _) => State::Skip,
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
    fn skips_long_sequences() {
        let mut long = b"\x1b]133;A".to_vec();
        long.extend(std::iter::repeat_n(b'x', MAX_PAYLOAD));
        long.extend(b"\x07\x1b]133;B\x07");
        assert_eq!(scan(&[&long]), [vec![(long.len(), Mark::CommandStart)]]);
    }
}
