param(
    [Parameter(Mandatory=$true)][string]$AvaloniaBuild,
    [Parameter(Mandatory=$true)][string]$WinUIBuild,
    [Parameter(Mandatory=$true)][string]$OutDir,
    [int]$WalkProcess = 0,
    [string]$WalkCheck = ''
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class W5Desktop {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct Startup { public int cb; public string reserved; public string desktop; public string title; public int x,y,xsize,ysize,xchars,ychars,fill,flags; public short show; public short reserved2; public IntPtr reserved3,stdin,stdout,stderr; }
    [StructLayout(LayoutKind.Sequential)]
    public struct ProcessInfo { public IntPtr process, thread; public int pid, tid; }
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateDesktopW(string name, IntPtr device, IntPtr mode, int flags, int access, IntPtr security);
    [DllImport("user32.dll", SetLastError=true)] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcessW(string app, string command, IntPtr pa, IntPtr ta, bool inherit, int flags, IntPtr env, string dir, ref Startup startup, out ProcessInfo info);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    static IntPtr desktop;
    public static int Start(string exe, string args) {
        if (desktop != IntPtr.Zero) throw new InvalidOperationException("HIDDEN_DESKTOP_IN_USE");
        desktop = CreateDesktopW("OpenPainter-W5-" + System.Diagnostics.Process.GetCurrentProcess().Id, IntPtr.Zero, IntPtr.Zero, 0, 0x000f01ff, IntPtr.Zero);
        if (desktop == IntPtr.Zero) throw new InvalidOperationException("CREATE_DESKTOP: " + Marshal.GetLastWin32Error());
        var startup = new Startup { cb = Marshal.SizeOf(typeof(Startup)), desktop = "OpenPainter-W5-" + System.Diagnostics.Process.GetCurrentProcess().Id };
        ProcessInfo info;
        if (!CreateProcessW(exe, "\""+exe+"\" "+args, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, System.IO.Path.GetDirectoryName(exe), ref startup, out info)) {
            var error = Marshal.GetLastWin32Error(); CloseDesktop(desktop); desktop = IntPtr.Zero;
            throw new InvalidOperationException("CREATE_HIDDEN_PROCESS: " + error);
        }
        CloseHandle(info.thread); CloseHandle(info.process);
        return info.pid;
    }
    public static int StartOnSame(string exe, string args) {
        if (desktop == IntPtr.Zero) throw new InvalidOperationException("HIDDEN_DESKTOP_NOT_OPEN");
        var startup = new Startup { cb = Marshal.SizeOf(typeof(Startup)), desktop = "OpenPainter-W5-" + System.Diagnostics.Process.GetCurrentProcess().Id };
        ProcessInfo info;
        if (!CreateProcessW(exe, "\""+exe+"\" "+args, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref startup, out info)) throw new InvalidOperationException("CREATE_WALKER: " + Marshal.GetLastWin32Error());
        CloseHandle(info.thread); CloseHandle(info.process);
        return info.pid;
    }
    public static void Close() { if (desktop != IntPtr.Zero) { if (!CloseDesktop(desktop)) throw new InvalidOperationException("CLOSE_DESKTOP: " + Marshal.GetLastWin32Error()); desktop = IntPtr.Zero; } }
}
'@
function Write-Check([string]$name, [bool]$passed, [object]$detail) {
    $file = Join-Path $OutDir "$name.json"
    @{ check=$name; passed=$passed; detail=$detail } | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath $file -Encoding utf8
    if ($passed) { Write-Host "$name PASS" } else { Write-Host "$name FAIL: $detail" }
}
function Find-Name([System.Windows.Automation.AutomationElement]$root, [string]$name) {
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $name)
    return $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
}
function Walk-Ui([int]$processId, [string]$check) {
    try {
        Start-Sleep -Seconds 2
        $pidCondition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $processId)
        $window = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, $pidCondition)
        if ($null -eq $window) { throw 'UIA_WINDOW_MISSING' }
        $names = @('Test menu', 'Floating panel', 'Canvas')
        foreach ($name in @('Test menu','Canvas')) { if ($null -eq (Find-Name $window $name)) { throw "UIA_MISSING_$($name.Replace(' ','_'))" } }
        $panel = Find-Name $window 'Floating panel'
        if ($null -eq $panel) {
            $panelButton = Find-Name $window 'Floating panel button'
            if ($null -eq $panelButton) { throw 'UIA_PANEL_MISSING' }
            ([System.Windows.Automation.InvokePattern]$panelButton.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)).Invoke()
            Start-Sleep -Milliseconds 400
            $panel = Find-Name ([System.Windows.Automation.AutomationElement]::RootElement) 'Floating panel'
            if ($null -eq $panel) { throw 'UIA_PANEL_MISSING' }
        }
        $menu = Find-Name $window 'Test menu'
        $pattern = $null
        if ($menu.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
            ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
        } elseif ($menu.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) {
            ([System.Windows.Automation.ExpandCollapsePattern]$pattern).Expand()
            $open = Find-Name $window 'Open dialog'
            if ($null -eq $open) { throw 'UIA_OPEN_DIALOG_MISSING' }
            ([System.Windows.Automation.InvokePattern]$open.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)).Invoke()
            Start-Sleep -Milliseconds 800
            # A WinUI menu stays open after UI Automation's Invoke, with its item focused. A person would press Enter.
            if ([System.Windows.Automation.AutomationElement]::FocusedElement.Current.Name -eq 'Open dialog') {
                Add-Type -AssemblyName System.Windows.Forms
                [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
            }
        } else { throw 'UIA_MENU_NOT_INVOKABLE' }
        $dialog = $null
        foreach ($try in 1..25) {
            Start-Sleep -Milliseconds 200
            $dialog = Find-Name $window 'Test dialog'
            if ($null -eq $dialog) { $dialog = Find-Name ([System.Windows.Automation.AutomationElement]::RootElement) 'Test dialog' }
            if ($null -ne $dialog) { break }
        }
        if ($null -eq $dialog) {
            $seen = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
            throw "UIA_DIALOG_MISSING; names seen: $($seen -join ' | ')"
        }
        if ($null -eq (Find-Name $dialog 'Number box')) { throw 'UIA_NUMBER_BOX_MISSING' }
        Write-Check $check $true $names
    } catch { Write-Check $check $false $_.Exception.Message }
}
# UI Automation sees only the desktop its thread is on, so a hidden walk runs as a second copy of this script there.
if ($WalkProcess -ne 0) { Walk-Ui $WalkProcess $WalkCheck; exit 0 }
function Run-Stack([string]$stack, [string]$build, [bool]$hidden) {
    $desk = if ($hidden) { 'hidden' } else { 'visible' }
    $check = "$stack-$desk-measure"
    $exe = Join-Path (Resolve-Path $build).Path $(if ($stack -eq 'avalonia') { 'SpikeAvalonia.exe' } else { 'SpikeWinUI.exe' })
    if (!(Test-Path -LiteralPath $exe)) { Write-Check $check $false 'EXE_MISSING'; return }
    $measure = Join-Path $OutDir "$stack-$desk-data.json"
    $owned = $null
    $uiaStarted = $false
    try {
        if ($hidden) { $id = [W5Desktop]::Start($exe, "--measure `"$measure`""); $owned = [Diagnostics.Process]::GetProcessById($id) }
        else { $owned = Start-Process -FilePath $exe -ArgumentList @('--measure', "`"$measure`"") -PassThru }
        # A process object loses its exit code unless its handle is opened while the process runs.
        $null = $owned.Handle
        $uiaStarted = $true
        if ($hidden) {
            $self = $PSCommandPath
            $walkArgs = "-NoProfile -ExecutionPolicy Bypass -File `"$self`" -AvaloniaBuild x -WinUIBuild x -OutDir `"$OutDir`" -WalkProcess $($owned.Id) -WalkCheck $stack-$desk-uia"
            $walker = [Diagnostics.Process]::GetProcessById([W5Desktop]::StartOnSame("$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe", $walkArgs))
            $null = $walker.Handle
            if (!$walker.WaitForExit(30000)) { Stop-Process -Id $walker.Id -Force; Write-Check "$stack-$desk-uia" $false 'UIA_WALK_TIMEOUT' }
            elseif (Test-Path -LiteralPath (Join-Path $OutDir "$stack-$desk-uia.json")) {
                $walked = Get-Content -LiteralPath (Join-Path $OutDir "$stack-$desk-uia.json") -Raw | ConvertFrom-Json
                if ($walked.passed) { Write-Host "$stack-$desk-uia PASS" } else { Write-Host "$stack-$desk-uia FAIL: $($walked.detail)" }
            } else { Write-Check "$stack-$desk-uia" $false "UIA_WALK_EXIT_$($walker.ExitCode)" }
        } else { Walk-Ui $owned.Id "$stack-$desk-uia" }
        if (!$owned.WaitForExit(30000)) { throw 'MEASURE_TIMEOUT' }
        if ($owned.ExitCode -ne 0) { throw "MEASURE_EXIT_$($owned.ExitCode)" }
        if (!(Test-Path -LiteralPath $measure)) { throw 'MEASURE_JSON_MISSING' }
        $data = Get-Content -LiteralPath $measure -Raw | ConvertFrom-Json
        if ($data.firstFrameMs -le 0 -or $data.frameTimesMs.Count -lt 2) { throw 'MEASURE_FRAMES_MISSING' }
        Write-Check $check $true @{ firstFrameMs=$data.firstFrameMs; frames=$data.frameTimesMs.Count; resizeFrames=$data.resizeFrames }
    } catch {
        if (!$uiaStarted) { Write-Check "$stack-$desk-uia" $false 'UIA_NOT_RUN_PROCESS_START_FAILED' }
        Write-Check $check $false $_.Exception.Message
    }
    finally {
        if ($null -ne $owned) {
            $owned.Refresh()
            if (!$owned.HasExited) { Stop-Process -Id $owned.Id -Force -ErrorAction Stop; $owned.WaitForExit(5000) | Out-Null }
            $owned.Dispose()
        }
        if ($hidden) { [W5Desktop]::Close() }
    }
}
Run-Stack 'avalonia' $AvaloniaBuild $false
Run-Stack 'avalonia' $AvaloniaBuild $true
Run-Stack 'winui' $WinUIBuild $false
Run-Stack 'winui' $WinUIBuild $true
