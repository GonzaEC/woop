# Woop shell integration for zsh.
#
# Woop points ZDOTDIR at this directory so that zsh reads this file, which
# loads the user's ~/.zshrc, restores ZDOTDIR and then marks the prompt,
# the command and its result with OSC 133 sequences so that Woop can group
# output into command blocks.

ZDOTDIR=${WOOP_USER_ZDOTDIR:-$HOME}
unset WOOP_USER_ZDOTDIR
if [[ -f "$ZDOTDIR/.zshrc" ]]; then
    source "$ZDOTDIR/.zshrc"
fi

if [[ -z $__woop_integrated ]]; then
    __woop_integrated=1

    __woop_save_status() {
        __woop_status=$?
    }

    # Percent-encodes $1, byte by byte, into REPLY, so that a directory
    # cannot end or change the mark it is sent in. No subshell is needed:
    # the locale set here is local to the function.
    __woop_encode() {
        emulate -L zsh
        local LC_ALL=C text=$1 char
        local -i i
        REPLY=
        for (( i = 1; i <= ${#text}; i++ )); do
            char=${text[i]}
            case $char in
                [a-zA-Z0-9.~_/-]) REPLY+=$char ;;
                *) printf -v char '%%%02X' "'$char"; REPLY+=$char ;;
            esac
        done
    }

    __woop_precmd() {
        if [[ -n $__woop_ran ]]; then
            print -n "\e]133;D;$__woop_status;woop=$__woop_token\a"
        fi
        __woop_ran=
        # The current directory, so that new panes open here. Left out when
        # too long for the mark, which would then be dropped as a whole.
        # Encoded again only when it changed.
        if [[ $PWD != "$__woop_pwd" ]]; then
            __woop_pwd=$PWD
            local REPLY
            __woop_encode "$PWD"
            __woop_directory=$REPLY
        fi
        local directory=$__woop_directory
        if (( ${#directory} > 3800 )); then
            directory=
        else
            # `%` starts a prompt escape in zsh: the encoding's are doubled.
            directory=";cwd=${directory//\%/%%}"
        fi
        # Take out the marks added at the last prompt, wherever they are now,
        # so that changes others made to the prompt stay (a virtual
        # environment's name), then wrap it again with the current directory.
        # A prompt a theme rebuilt, or with its own unsigned marks, is simply
        # wrapped: Woop ignores unsigned marks.
        if [[ -n $__woop_prefix ]]; then
            PS1=${PS1/"$__woop_prefix"/}
            PS1=${PS1/"$__woop_suffix"/}
        fi
        __woop_prefix=$'%{\e]133;A;woop='"$__woop_token$directory"$'\a%}'
        __woop_suffix=$'%{\e]133;B;woop='"$__woop_token"$'\a%}'
        PS1=$__woop_prefix$PS1$__woop_suffix
    }

    __woop_preexec() {
        __woop_ran=1
        print -n "\e]133;C;woop=$__woop_token\a"
    }

    # The status must be read before other hooks run, and the prompt
    # wrapped after they have built it.
    precmd_functions=(__woop_save_status $precmd_functions __woop_precmd)
    preexec_functions+=(__woop_preexec)
fi
