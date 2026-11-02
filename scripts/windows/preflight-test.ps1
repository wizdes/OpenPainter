$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'preflight.ps1'
function Check($name, $level, $facts, $wantPass, $wantText) {
    $output = & $script -Level $level -Facts $facts 2>&1
    $code = $LASTEXITCODE
    if (($code -eq 0) -ne $wantPass) { throw "$name exit $code; output: $output" }
    if (($output -join "`n") -notmatch [regex]::Escape($wantText)) { throw "$name missing $wantText; output: $output" }
    Write-Output "PASS $name"
}
$base = @{ DotNet10=$true; Cargo=$true; DiskGB=41; UserSession=$true; Unlocked=$true; PaintVersion='5.112.9563.32325'; SunshineRunning=$true; EncoderSessions=0; LMStudioRunning=$false; LoadedModels=$null }
Check 'fast pass' fast $base $true 'PREFLIGHT: PASS'
$low = $base.Clone(); $low.DiskGB = 39
Check 'disk low fails' fast $low $false '40 GB'
$unknown = $base.Clone(); $unknown.Cargo = $null
Check 'unknown fails' fast $unknown $false 'cannot determine'
$ssh = $base.Clone(); $ssh.UserSession = $false
Check 'SSH window fails' window $ssh $false 'UI TESTS: NOT RUN'
$locked = $base.Clone(); $locked.Unlocked = $false
Check 'locked window fails' window $locked $false 'UI TESTS: NOT RUN'
$pin = $base.Clone(); $pin.PaintVersion = 'wrong'
Check 'wrong pin fails' checkpoint $pin $false 'paint.net pin'
Check 'Sunshine with no stream passes' speed $base $true 'PREFLIGHT: PASS'
$stream = $base.Clone(); $stream.EncoderSessions = 1
Check 'stream fails' speed $stream $false 'Sunshine idle: FAIL'
$noSmi = $base.Clone(); $noSmi.EncoderSessions = $null
Check 'unknown stream fails' speed $noSmi $false 'Sunshine idle: FAIL (cannot determine)'
$noSun = $noSmi.Clone(); $noSun.SunshineRunning = $false
Check 'no Sunshine passes without nvidia-smi' speed $noSun $true 'Sunshine idle: PASS'
$unused = $base.Clone(); $unused.LMStudioRunning = $true; $unused.LoadedModels = 0
Check 'LM Studio with no model passes' checkpoint $unused $true 'LM Studio idle: PASS'
$model = $unused.Clone(); $model.LoadedModels = 2
Check 'loaded model fails' checkpoint $model $false 'LM Studio idle: FAIL'
$noApi = $unused.Clone(); $noApi.LoadedModels = $null
Check 'unknown model fails' speed $noApi $false 'LM Studio idle: FAIL (cannot determine)'
Write-Output 'PREFLIGHT TESTS: PASS'
