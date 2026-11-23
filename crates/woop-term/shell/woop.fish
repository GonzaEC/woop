# Woop shell integration for fish.
#
# Passed with --init-command, so it runs after the user's configuration.
# Marks the prompt, the command and its result with OSC 133 sequences so
# that Woop can group output into command blocks.

if not set -q __woop_integrated
    set -g __woop_integrated 1

    # Woop passes a per-session token that every mark carries, so that
    # output cannot forge marks. Keep it in a global (not exported) variable
    # and remove it from the environment, so programs do not inherit it.
    set -g __woop_token (string replace -ra '[^0-9a-f]' '' -- "$WOOP_TOKEN")
    set -e WOOP_TOKEN

    function __woop_preexec --on-event fish_preexec
        printf '\e]133;C;woop=%s\a' "$__woop_token"
    end

    function __woop_postexec --on-event fish_postexec
        printf '\e]133;D;%s;woop=%s\a' $status "$__woop_token"
    end

    function __woop_return
        return $argv[1]
    end

    functions -q fish_prompt
    and functions -c fish_prompt __woop_original_prompt

    function fish_prompt
        set -l last_status $status
        # The current directory, percent-encoded, so that new panes open here;
        # left out when too long for the mark.
        set -l directory (string escape --style=url -- "$PWD")
        if test (string length -- "$directory") -le 3800
            printf '\e]133;A;woop=%s;cwd=%s\a' "$__woop_token" "$directory"
        else
            printf '\e]133;A;woop=%s\a' "$__woop_token"
        end
        if functions -q __woop_original_prompt
            # Restore the status so the original prompt can show it.
            __woop_return $last_status
            __woop_original_prompt
        end
        printf '\e]133;B;woop=%s\a' "$__woop_token"
    end
end
