# Woop shell integration for bash.
#
# Loaded with --rcfile in place of ~/.bashrc, which it sources first. Marks
# the prompt, the command and its result with OSC 133 sequences so that Woop
# can group output into command blocks.

# Woop passes a per-session token that every mark carries, so that output
# cannot forge marks. Keep it in a shell variable and remove it from the
# environment before anything else runs, so programs do not inherit it.
# Only hexadecimal digits are kept: the token is written into PS1, where
# anything else could be expanded.
__woop_token=${WOOP_TOKEN//[^0-9a-f]/}
unset WOOP_TOKEN

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

    # Percent-encodes $1, byte by byte, so that a directory cannot end or
    # change the mark it is sent in. Called only in a command substitution,
    # so the locale set here stays in that subshell.
    __woop_encode() {
        # Read bytes, not characters. bash 3.2 (macOS) may keep reading
        # characters even so (it ignored `local LC_ALL=C`), which the probe
        # tells: each character's UTF-8 bytes are then encoded here.
        LC_ALL=C
        local text=$1 encoded= char code byte i probe=$'\xc3\xb1' bytes
        local by_character=
        [ "${#probe}" -eq 1 ] && by_character=1
        for (( i = 0; i < ${#text}; i++ )); do
            char=${text:i:1}
            case $char in
                [a-zA-Z0-9.~_/-]) encoded+=$char; continue ;;
            esac
            printf -v code '%d' "'$char"
            if [ -z "$by_character" ] || (( code < 0x80 )); then
                bytes=$(( code & 0xFF ))
            elif (( code < 0x800 )); then
                bytes="$(( 0xC0 | code >> 6 )) $(( 0x80 | (code & 0x3F) ))"
            elif (( code < 0x10000 )); then
                bytes="$(( 0xE0 | code >> 12 )) $(( 0x80 | (code >> 6 & 0x3F) )) $(( 0x80 | (code & 0x3F) ))"
            else
                bytes="$(( 0xF0 | code >> 18 )) $(( 0x80 | (code >> 12 & 0x3F) )) $(( 0x80 | (code >> 6 & 0x3F) )) $(( 0x80 | (code & 0x3F) ))"
            fi
            for byte in $bytes; do
                printf -v char '%%%02X' "$byte"
                encoded+=$char
            done
        done
        printf '%s' "$encoded"
    }

    __woop_prompt() {
        if [ -n "$__woop_prompted" ]; then
            printf '\033]133;D;%s;woop=%s\007' "$__woop_status" "$__woop_token"
        fi
        __woop_prompted=1
        __woop_at_prompt=1
        # The current directory, so that new panes open here. Left out when
        # too long for the mark, which would then be dropped as a whole.
        local directory
        directory=$(__woop_encode "$PWD")
        if [ "${#directory}" -gt 3800 ]; then
            directory=
        else
            directory=";cwd=$directory"
        fi
        # Take out the marks added at the last prompt, wherever they are now,
        # so that changes others made to the prompt stay (a virtual
        # environment's name), then wrap it again with the current directory.
        # A prompt rebuilt from scratch, or with its own unsigned marks, is
        # simply wrapped: Woop ignores unsigned marks.
        if [ -n "$__woop_prefix" ]; then
            PS1=${PS1/"$__woop_prefix"/}
            PS1=${PS1/"$__woop_suffix"/}
        fi
        __woop_prefix='\[\033]133;A;woop='"$__woop_token$directory"'\007\]'
        __woop_suffix='\[\033]133;B;woop='"$__woop_token"'\007\]'
        PS1=$__woop_prefix$PS1$__woop_suffix
        return "$__woop_status"
    }

    PROMPT_COMMAND="__woop_save_status;${PROMPT_COMMAND:+$PROMPT_COMMAND;}__woop_prompt"

    if [ "${BASH_VERSINFO[0]}" -gt 4 ] || { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -ge 4 ]; }; then
        # PS0 is printed after a command is read and before it runs.
        PS0='\033]133;C;woop='"$__woop_token"'\007'"$PS0"
    elif [ -z "$(trap -p DEBUG)" ]; then
        # Older versions have no PS0; use the DEBUG trap, which runs before
        # every command, and only mark the first one after a prompt. A DEBUG
        # trap the user already set is left alone: commands are then not
        # marked, but the user's configuration keeps working.
        __woop_preexec() {
            if [ -n "$__woop_at_prompt" ]; then
                __woop_at_prompt=
                printf '\033]133;C;woop=%s\007' "$__woop_token"
            fi
        }
        trap '__woop_preexec' DEBUG
    fi
fi
