# Woop shell integration for zsh: loads the user's ~/.zshenv.
#
# Woop points ZDOTDIR at this directory so that zsh reads its files, and
# keeps the user's own ZDOTDIR in WOOP_USER_ZDOTDIR.

# Woop passes a per-session token that every mark carries, so that output
# cannot forge marks. Keep it in a shell variable and remove it from the
# environment before anything else runs, so programs do not inherit it.
# Only hexadecimal digits are kept: the token is written into PS1.
__woop_token=${WOOP_TOKEN//[^0-9a-f]/}
unset WOOP_TOKEN

if [[ -f "$WOOP_USER_ZDOTDIR/.zshenv" ]]; then
    __woop_zdotdir=$ZDOTDIR
    ZDOTDIR=$WOOP_USER_ZDOTDIR
    source "$WOOP_USER_ZDOTDIR/.zshenv"
    # The user's file may itself change ZDOTDIR.
    WOOP_USER_ZDOTDIR=$ZDOTDIR
    ZDOTDIR=$__woop_zdotdir
    unset __woop_zdotdir
fi
