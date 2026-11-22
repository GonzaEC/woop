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

    __woop_precmd() {
        if [[ -n $__woop_ran ]]; then
            print -n "\e]133;D;$__woop_status\a"
        fi
        __woop_ran=
        # Wrap the prompt again if a theme rebuilt it.
        if [[ $PS1 != *'133;A'* ]]; then
            PS1=$'%{\e]133;A\a%}'"$PS1"$'%{\e]133;B\a%}'
        fi
    }

    __woop_preexec() {
        __woop_ran=1
        print -n "\e]133;C\a"
    }

    # The status must be read before other hooks run, and the prompt
    # wrapped after they have built it.
    precmd_functions=(__woop_save_status $precmd_functions __woop_precmd)
    preexec_functions+=(__woop_preexec)
fi
