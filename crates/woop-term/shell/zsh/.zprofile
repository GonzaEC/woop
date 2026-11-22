# Woop shell integration for zsh: loads the user's ~/.zprofile.
#
# Woop points ZDOTDIR at this directory so that zsh reads its files, and
# keeps the user's own ZDOTDIR in WOOP_USER_ZDOTDIR.

if [[ -f "$WOOP_USER_ZDOTDIR/.zprofile" ]]; then
    __woop_zdotdir=$ZDOTDIR
    ZDOTDIR=$WOOP_USER_ZDOTDIR
    source "$WOOP_USER_ZDOTDIR/.zprofile"
    # The user's file may itself change ZDOTDIR.
    WOOP_USER_ZDOTDIR=$ZDOTDIR
    ZDOTDIR=$__woop_zdotdir
    unset __woop_zdotdir
fi
