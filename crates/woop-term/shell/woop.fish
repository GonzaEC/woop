# Woop shell integration for fish.
#
# Passed with --init-command, so it runs after the user's configuration.
# Marks the prompt, the command and its result with OSC 133 sequences so
# that Woop can group output into command blocks.

if not set -q __woop_integrated
    set -g __woop_integrated 1

    function __woop_preexec --on-event fish_preexec
        printf '\e]133;C\a'
    end

    function __woop_postexec --on-event fish_postexec
        printf '\e]133;D;%s\a' $status
    end

    function __woop_return
        return $argv[1]
    end

    functions -q fish_prompt
    and functions -c fish_prompt __woop_original_prompt

    function fish_prompt
        set -l last_status $status
        printf '\e]133;A\a'
        if functions -q __woop_original_prompt
            # Restore the status so the original prompt can show it.
            __woop_return $last_status
            __woop_original_prompt
        end
        printf '\e]133;B\a'
    end
end
