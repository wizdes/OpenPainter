# W5: press Tab through a shell and read what UI Automation reports as focused at each stop.
# That is the name a screen reader is given. Run as the signed-in user, on the visible desktop.
# It also opens the dialog through the menu and reads the dialog's names. It stops only its own process.
param([Parameter(Mandatory=$true)][string]$Exe, [Parameter(Mandatory=$true)][string]$Shot)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class W5Fg {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr w);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr w, out RECT r);
}
'@
$ua = [System.Windows.Automation.AutomationElement]
$before = [W5Fg]::GetForegroundWindow()
$p = Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe) -PassThru
try {
    Start-Sleep -Seconds 3
    $p.Refresh()
    [W5Fg]::keybd_event(0x12, 0, 0, [IntPtr]::Zero); [W5Fg]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)
    [void][W5Fg]::SetForegroundWindow($p.MainWindowHandle)
    Start-Sleep -Milliseconds 500
    $stops = @()
    foreach ($i in 1..6) {
        [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
        Start-Sleep -Milliseconds 400
        $f = $ua::FocusedElement
        $stops += "$($f.Current.Name) [$($f.Current.ControlType.ProgrammaticName -replace 'ControlType\.','')]"
    }
    "tab stops: " + ($stops -join ' -> ')
    $window = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
    $byName = { param($root, $name) $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.PropertyCondition]::new($ua::NameProperty, $name)) }
    $menu = & $byName $window 'Test menu'
    $pattern = $null
    if ($menu.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { $pattern.Invoke() }
    else {
        $menu.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        Start-Sleep -Milliseconds 500
        (& $byName $window 'Open dialog').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    }
    Start-Sleep -Seconds 1
    # UI Automation's Invoke leaves a WinUI menu open with its item focused, so try the key a person would press.
    if ($ua::FocusedElement.Current.Name -eq 'Open dialog') {
        "Invoke did not open the dialog; pressing Enter"
        [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    }
    Start-Sleep -Seconds 2
    $r = New-Object W5Fg+RECT
    [void][W5Fg]::GetWindowRect($p.MainWindowHandle, [ref]$r)
    $b = New-Object Drawing.Bitmap ($r.r - $r.l), ($r.b - $r.t)
    $g = [Drawing.Graphics]::FromImage($b); $g.CopyFromScreen($r.l, $r.t, 0, 0, $b.Size); $b.Save($Shot); $g.Dispose(); $b.Dispose()
    $names = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    "names in main window after opening the dialog: " + ($names -join ' | ')
    $all = @($ua::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.PropertyCondition]::new($ua::ProcessIdProperty, $p.Id)) | ForEach-Object { $_.Current.Name })
    "top-level windows: " + ($all -join ' | ')
    $f = $ua::FocusedElement
    "focused after opening: $($f.Current.Name) [$($f.Current.ControlType.ProgrammaticName)]"
} finally {
    $p.Refresh()
    if (!$p.HasExited) { Stop-Process -Id $p.Id -Force }
    [void][W5Fg]::SetForegroundWindow($before)
}
