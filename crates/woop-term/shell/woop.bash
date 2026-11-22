# Woop shell integration for bash.
#
# Loaded with --rcfile in place of ~/.bashrc, which it sources first. Marks
# the prompt, the command and its result with OSC 133 sequences so that Woop
# can group output into command blocks.

if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

if [ -z "$__woop_integrated" ]; then
    __woop_integrated=1

    # Returns the status it saves, so the user's PROMPT_COMMAND, which runs
    # next, still sees the last command's exit status in $?.
    __woop_save_status() {
        __woop_status=$?
        return "$__woop_status"
    }

    __woop_prompt() {
        if [ -n "$__woop_prompted" ]; then
            printf '\033]133;D;%s\007' "$__woop_status"
        fi
        __woop_prompted=1
        __woop_at_prompt=1
        # Wrap the prompt again if the user's PROMPT_COMMAND rebuilt it.
        case "$PS1" in
            *'133;A'*) ;;
            *) PS1='\[\033]133;A\007\]'"$PS1"'\[\033]133;B\007\]' ;;
        esac
        return "$__woop_status"
    }

    PROMPT_COMMAND="__woop_save_status;${PROMPT_COMMAND:+$PROMPT_COMMAND;}__woop_prompt"

    if [ "${BASH_VERSINFO[0]}" -gt 4 ] || { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -ge 4 ]; }; then
        # PS0 is printed after a command is read and before it runs.
        PS0='\033]133;C\007'"$PS0"
    elif [ -z "$(trap -p DEBUG)" ]; then
        # Older versions have no PS0; use the DEBUG trap, which runs before
        # every command, and only mark the first one after a prompt. A DEBUG
        # trap the user already set is left alone: commands are then not
        # marked, but the user's configuration keeps working.
        __woop_preexec() {
            if [ -n "$__woop_at_prompt" ]; then
                __woop_at_prompt=
                printf '\033]133;C\007'
            fi
        }
        trap '__woop_preexec' DEBUG
    fi
fi
