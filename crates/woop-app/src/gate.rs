//! When the command editor takes the keys (ADR 0016, 0017).
//!
//! The editor turns on when the shell waits for a command with an empty
//! line, and then stays on for that prompt even if output moves the cursor:
//! switching keys back to the shell halfway through a command would send
//! the rest of it on its own. It turns off when the command is submitted or
//! handed off, when output starts, at the next prompt, or on the alternate
//! screen.

/// What the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The editor takes the keys.
    Active,
    /// As `Active`, after Enter was refused once because the shell's line
    /// might not be empty any more; Enter again sends anyway.
    Confirm,
    /// The prompt is shown but belongs to the shell's own line editor: it
    /// was handed off, or the shell's line already held text.
    ShellLine,
    /// No prompt: a command is running (or the shell has not reported one).
    Running,
}

/// What to do when Enter is pressed in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submit {
    /// Send the command.
    Send,
    /// Do not send yet: ask the user to press Enter again.
    Confirm,
}

/// How the editor gave up its prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumed {
    /// The command was sent and is about to run.
    Submitted,
    /// The text was typed into the shell's own line.
    HandedOff,
}

/// Tracks which prompt the editor belongs to.
#[derive(Debug, Default)]
pub struct Gate {
    /// Prompt the editor is on for.
    armed: Option<u64>,
    /// Prompt whose command was submitted or handed off, and how; the
    /// editor does not turn on again for it.
    consumed: Option<(u64, Consumed)>,
    /// Enter was refused once for the armed prompt.
    confirm: bool,
}

impl Gate {
    /// Updates the gate from a new screen. `awaiting` is the prompt waiting
    /// with an empty line, `prompt` the prompt shown whatever the line
    /// holds. Returns whether the editor just turned on.
    pub fn observe(&mut self, awaiting: Option<u64>, prompt: Option<u64>) -> bool {
        if self.armed.is_some() && self.armed != prompt {
            self.armed = None;
            self.confirm = false;
        }
        if self.consumed.is_some_and(|(id, _)| Some(id) != prompt) {
            self.consumed = None;
        }
        if self.armed.is_none()
            && awaiting.is_some()
            && awaiting == prompt
            && awaiting != self.consumed.map(|(id, _)| id)
        {
            self.armed = awaiting;
            self.confirm = false;
            return true;
        }
        false
    }

    /// Whether the editor takes the keys.
    pub fn is_active(&self) -> bool {
        self.armed.is_some()
    }

    /// What the panel shows, given the prompt on screen.
    pub fn status(&self, prompt: Option<u64>) -> Status {
        match (self.armed, self.confirm, self.consumed) {
            (Some(_), false, _) => Status::Active,
            (Some(_), true, _) => Status::Confirm,
            (None, _, _) if prompt.is_none() => Status::Running,
            (None, _, Some((id, Consumed::Submitted))) if Some(id) == prompt => Status::Running,
            (None, _, _) => Status::ShellLine,
        }
    }

    /// Decides what Enter does. Sending is safe while the shell's line is
    /// still empty (`awaiting` is the armed prompt); otherwise the user
    /// confirms with a second Enter. Nothing is sent while the editor is off.
    pub fn submit(&mut self, awaiting: Option<u64>) -> Submit {
        if self.armed.is_none() {
            return Submit::Confirm;
        }
        if awaiting == self.armed || self.confirm {
            Submit::Send
        } else {
            self.confirm = true;
            Submit::Confirm
        }
    }

    /// The text changed: a pending confirmation no longer applies.
    pub fn edited(&mut self) {
        self.confirm = false;
    }

    /// The command was sent or handed off: the editor turns off until the
    /// next prompt.
    pub fn consume(&mut self, how: Consumed) {
        self.consumed = self.armed.take().map(|id| (id, how));
        self.confirm = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_on_at_an_empty_prompt_only() {
        let mut gate = Gate::default();
        assert!(!gate.observe(None, None));
        assert_eq!(gate.status(None), Status::Running);
        // Text already in the shell's line (typed ahead).
        assert!(!gate.observe(None, Some(1)));
        assert!(!gate.is_active());

        assert!(gate.observe(Some(1), Some(1)));
        assert!(gate.is_active());
        assert_eq!(gate.status(Some(1)), Status::Active);
        assert!(!gate.observe(Some(1), Some(1)), "turns on only once");
    }

    #[test]
    fn stays_on_when_output_moves_the_cursor() {
        let mut gate = Gate::default();
        gate.observe(Some(1), Some(1));
        // A background job prints a line: the line no longer looks empty.
        gate.observe(None, Some(1));
        assert!(gate.is_active(), "keys must not switch to the shell");
    }

    #[test]
    fn turns_off_when_output_starts_or_a_new_prompt_appears() {
        let mut gate = Gate::default();
        gate.observe(Some(1), Some(1));
        gate.observe(None, None);
        assert!(!gate.is_active());

        gate.observe(Some(2), Some(2));
        gate.observe(Some(3), Some(3));
        assert!(gate.is_active(), "on for the new prompt");
        assert_eq!(gate.armed, Some(3));
    }

    #[test]
    fn submits_at_once_while_the_line_is_empty() {
        let mut gate = Gate::default();
        gate.observe(Some(1), Some(1));
        assert_eq!(gate.submit(Some(1)), Submit::Send);
    }

    #[test]
    fn asks_to_confirm_when_the_line_may_not_be_empty() {
        let mut gate = Gate::default();
        gate.observe(Some(1), Some(1));
        gate.observe(None, Some(1));

        assert_eq!(gate.submit(None), Submit::Confirm);
        assert_eq!(gate.status(Some(1)), Status::Confirm);
        // Editing withdraws the confirmation.
        gate.edited();
        assert_eq!(gate.status(Some(1)), Status::Active);
        assert_eq!(gate.submit(None), Submit::Confirm);
        assert_eq!(gate.submit(None), Submit::Send);
    }

    #[test]
    fn never_sends_while_off() {
        let mut gate = Gate::default();
        assert_eq!(gate.submit(Some(1)), Submit::Confirm);
        assert_eq!(gate.submit(Some(1)), Submit::Confirm);
        // A refusal while off does not carry over to the next prompt: its
        // first Enter is still checked.
        gate.observe(Some(1), Some(1));
        gate.observe(None, Some(1));
        assert_eq!(gate.status(Some(1)), Status::Active);
        assert_eq!(gate.submit(None), Submit::Confirm);
    }

    #[test]
    fn a_submitted_prompt_shows_the_command_starting() {
        let mut gate = Gate::default();
        gate.observe(Some(1), Some(1));
        gate.consume(Consumed::Submitted);
        assert!(!gate.is_active());
        assert_eq!(gate.status(Some(1)), Status::Running);
    }

    #[test]
    fn text_typed_ahead_leaves_the_prompt_to_the_shell() {
        let mut gate = Gate::default();
        assert!(!gate.observe(None, Some(1)));
        assert_eq!(gate.status(Some(1)), Status::ShellLine);
    }

    #[test]
    fn the_alternate_screen_turns_the_editor_off_until_the_next_prompt() {
        let mut gate = Gate::default();
        gate.observe(Some(1), Some(1));
        // A full-screen program hides the prompt.
        gate.observe(None, None);
        assert!(!gate.is_active());
        // Back at the same, still empty prompt: the editor turns on again.
        assert!(gate.observe(Some(1), Some(1)));
    }

    #[test]
    fn a_consumed_prompt_is_left_to_the_shell() {
        let mut gate = Gate::default();
        gate.observe(Some(1), Some(1));
        gate.consume(Consumed::HandedOff);
        assert!(!gate.is_active());
        assert_eq!(gate.status(Some(1)), Status::ShellLine);
        // The shell's line is empty again (the user cleared it there): the
        // prompt still belongs to the shell.
        assert!(!gate.observe(Some(1), Some(1)));
        assert!(!gate.is_active());
        // While the command runs, then at the next prompt.
        gate.observe(None, None);
        assert_eq!(gate.status(None), Status::Running);
        assert!(gate.observe(Some(2), Some(2)));
    }
}
