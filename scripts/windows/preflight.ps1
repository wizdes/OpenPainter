param(
    [ValidateSet('fast','milestone','window','checkpoint','speed','capture')][string]$Level = 'fast',
    [hashtable]$Facts
)
$ErrorActionPreference = 'Stop'
function Get-LiveFacts {
    $f = @{}
    try {
        $dotnet = Join-Path $env:USERPROFILE '.dotnet10\dotnet.exe'
        $sdk = Join-Path $env:USERPROFILE '.dotnet10\sdk'
        $f.DotNet10 = (Test-Path -LiteralPath $dotnet -PathType Leaf) -and
            ([Diagnostics.FileVersionInfo]::GetVersionInfo($dotnet).FileMajorPart -eq 10) -and
            (@(Get-ChildItem -LiteralPath $sdk -Directory -ErrorAction Stop | Where-Object { $_.Name -match '^10\.' }).Count -gt 0)
    } catch { $f.DotNet10 = $null }
    try { $f.Cargo = $null -ne (Get-Command cargo -ErrorAction Stop) } catch { $f.Cargo = $null }
    try {
        $drive = New-Object IO.DriveInfo 'C:'
        if (-not $drive.IsReady) { $f.DiskGB = $null }
        else { $f.DiskGB = [double]$drive.AvailableFreeSpace / 1GB }
    } catch { $f.DiskGB = $null }
    try {
        if ($env:SSH_CONNECTION -or $env:SSH_CLIENT -or $env:SSH_TTY) { $f.UserSession = $false; $f.Unlocked = $null }
        else {
            if (-not ('PreflightDesktop' -as [type])) {
                Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PreflightDesktop {
 [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
 [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
 [DllImport("user32.dll", SetLastError=true)] public static extern bool GetUserObjectInformation(IntPtr handle, int index, StringBuilder info, uint length, out uint needed);
 [DllImport("user32.dll", SetLastError=true)] public static extern bool CloseDesktop(IntPtr handle);
}
"@
            }
            $active = [PreflightDesktop]::WTSGetActiveConsoleSessionId()
            $f.UserSession = ($active -ne [uint32]::MaxValue -and [Diagnostics.Process]::GetCurrentProcess().SessionId -eq $active)
            if (-not $f.UserSession) { $f.Unlocked = $null }
            else {
                $handle = [PreflightDesktop]::OpenInputDesktop(0, $false, 1)
                if ($handle -eq [IntPtr]::Zero) { $f.Unlocked = $null }
                else {
                    try {
                        $name = New-Object Text.StringBuilder 128
                        [uint32]$needed = 0
                        if ([PreflightDesktop]::GetUserObjectInformation($handle, 2, $name, 256, [ref]$needed)) { $f.Unlocked = ($name.ToString() -eq 'Default') }
                        else { $f.Unlocked = $null }
                    } finally { [void][PreflightDesktop]::CloseDesktop($handle) }
                }
            }
        }
    } catch { $f.UserSession = $null; $f.Unlocked = $null }
    try {
        $pin = 'C:\agent-work\pdn-5.1.12\paintdotnet.exe'
        if (Test-Path -LiteralPath $pin -PathType Leaf) { $f.PaintVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($pin).FileVersion }
        else { $f.PaintVersion = '' }
    } catch { $f.PaintVersion = $null }
    try {
        $pinRoot = [IO.Path]::GetFullPath('C:\agent-work\pdn-5.1.12') + [IO.Path]::DirectorySeparatorChar
        $paintProcesses = @(Get-CimInstance Win32_Process -Filter "Name LIKE 'paintdotnet%'" -ErrorAction Stop)
        $f.OutsidePinPaint = $false
        foreach ($process in $paintProcesses) {
            if ([string]::IsNullOrWhiteSpace($process.ExecutablePath)) { $f.OutsidePinPaint = $null; break }
            $path = [IO.Path]::GetFullPath($process.ExecutablePath)
            if (-not $path.StartsWith($pinRoot, [StringComparison]::OrdinalIgnoreCase)) { $f.OutsidePinPaint = $true; break }
        }
    } catch { $f.OutsidePinPaint = $null }
    # Sunshine stays running when nobody streams. A stream holds an NVENC session.
    try { $f.SunshineRunning = @((Get-Process -Name sunshine -ErrorAction SilentlyContinue)).Count -gt 0 } catch { $f.SunshineRunning = $null }
    $f.EncoderSessions = $null
    if ($f.SunshineRunning) {
        try {
            $raw = & nvidia-smi --query-gpu=encoder.stats.sessionCount --format=csv,noheader 2>&1
            if ($LASTEXITCODE -eq 0) { $f.EncoderSessions = [int](@($raw)[0].ToString().Trim()) }
        } catch { $f.EncoderSessions = $null }
    }
    # The lms command would wake LM Studio, so it is off limits here. Ask its server only.
    try { $f.LMStudioRunning = @((Get-Process -Name 'LM Studio','LMStudio' -ErrorAction SilentlyContinue)).Count -gt 0 } catch { $f.LMStudioRunning = $null }
    $f.LoadedModels = $null
    if ($f.LMStudioRunning) {
        try {
            $body = (Invoke-WebRequest -UseBasicParsing -TimeoutSec 3 http://127.0.0.1:1234/api/v0/models).Content | ConvertFrom-Json
            $f.LoadedModels = @($body.data | Where-Object { $_.state -eq 'loaded' }).Count
        } catch { $f.LoadedModels = $null }
    }
    return $f
}
function Resolve-Idle($running, $count) {
    if ($null -eq $running) { return $null }
    if (-not $running) { return $true }
    if ($null -eq $count) { return $null }
    return ([int]$count -eq 0)
}
if ($null -eq $Facts) { $Facts = Get-LiveFacts }
$reasons = New-Object 'System.Collections.Generic.List[string]'
function Check-Boolean($name, $value) {
    if ($null -eq $value) { Write-Output "$name`: FAIL (cannot determine)"; $reasons.Add("$name cannot determine") }
    elseif ($value -eq $true) { Write-Output "$name`: PASS" }
    else { Write-Output "$name`: FAIL"; $reasons.Add("$name failed") }
}
Check-Boolean '.NET 10' $Facts.DotNet10
Check-Boolean 'cargo' $Facts.Cargo
if ($null -eq $Facts.DiskGB) { Write-Output 'C: 40 GB free: FAIL (cannot determine)'; $reasons.Add('disk cannot determine') }
elseif ([double]$Facts.DiskGB -ge 40) { Write-Output 'C: 40 GB free: PASS' }
else { Write-Output 'C: 40 GB free: FAIL'; $reasons.Add('under 40 GB free') }
if ($Level -in @('window','checkpoint','speed')) {
    $uiReason = $null
    if ($null -eq $Facts.UserSession) { $uiReason = 'cannot determine desktop session' }
    elseif (-not $Facts.UserSession) { $uiReason = 'not the active user desktop session' }
    elseif ($null -eq $Facts.Unlocked) { $uiReason = 'cannot determine lock state' }
    elseif (-not $Facts.Unlocked) { $uiReason = 'desktop is locked' }
    if ($uiReason) { Write-Output "UI TESTS: NOT RUN ($uiReason)"; $reasons.Add("UI tests: $uiReason") }
    else { Write-Output 'UI TESTS: PASS' }
}
if ($Level -in @('checkpoint','speed','capture')) {
    if ($null -eq $Facts.PaintVersion) { Write-Output 'paint.net pin: FAIL (cannot determine)'; $reasons.Add('paint.net pin cannot determine') }
    elseif ($Facts.PaintVersion -eq '5.112.9563.32325') { Write-Output 'paint.net pin: PASS' }
    else { Write-Output 'paint.net pin: FAIL'; $reasons.Add('paint.net pin wrong or absent') }
}
if ($Level -in @('checkpoint','speed')) {
    Check-Boolean 'Sunshine idle' (Resolve-Idle $Facts.SunshineRunning $Facts.EncoderSessions)
    Check-Boolean 'LM Studio idle' (Resolve-Idle $Facts.LMStudioRunning $Facts.LoadedModels)
}
if ($Level -in @('checkpoint','speed','capture')) { Check-Boolean 'paint.net outside pin' $(if ($null -eq $Facts.OutsidePinPaint) { $null } else { -not $Facts.OutsidePinPaint }) }
if ($reasons.Count -eq 0) { Write-Output 'PREFLIGHT: PASS'; exit 0 }
Write-Output ('PREFLIGHT: FAIL (' + ($reasons -join '; ') + ')')
exit 1
