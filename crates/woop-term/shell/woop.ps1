# Woop shell integration for PowerShell (Windows PowerShell 5.1 and PowerShell 7).
#
# Marks the prompt, the command and its result with OSC 133 sequences so
# that Woop can group output into command blocks. Loaded by Woop after the
# user's profile; it wraps the existing prompt instead of replacing it.

if ($Global:__WoopIntegrated) { return }
$Global:__WoopIntegrated = $true

function Global:__WoopMark([string] $Payload) {
    "$([char]27)]133;$Payload$([char]7)"
}

$Global:__WoopOriginalPrompt = $function:prompt
$Global:__WoopFirstPrompt = $true

function Global:prompt {
    # Read the result of the last command before anything else changes it.
    $success = $?
    $exitCode = $Global:LASTEXITCODE

    $marks = ''
    if (-not $Global:__WoopFirstPrompt) {
        $status = if ($success) { 0 } elseif ($exitCode) { $exitCode } else { 1 }
        $marks += __WoopMark "D;$status"
    }
    $Global:__WoopFirstPrompt = $false

    $text = & $Global:__WoopOriginalPrompt
    $Global:LASTEXITCODE = $exitCode
    "$marks$(__WoopMark 'A')$text$(__WoopMark 'B')"
}

# PSReadLine reads the command line; mark the start of the output once it
# returns the line to run.
if (-not (Get-Module PSReadLine)) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
}
if (Test-Path Function:\PSConsoleHostReadLine) {
    $Global:__WoopOriginalReadLine = $function:PSConsoleHostReadLine
    function Global:PSConsoleHostReadLine {
        $line = & $Global:__WoopOriginalReadLine
        [Console]::Write((__WoopMark 'C'))
        $line
    }
}
