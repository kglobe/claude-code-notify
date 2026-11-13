param(
    [ValidateSet('notify', 'input', 'stack', 'doctor')]
    [string]$Mode = 'notify',
    [string]$Exe = "$PSScriptRoot\..\..\..\src-rust\target\release\toast-window.exe",
    [string]$Message = '',
    [string]$Title = '',
    [string]$Cwd = 'D:\GitHub\claude-code-notify',
    [string]$OutDir = "$PSScriptRoot\..\..\..\verify-evidence"
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
# Stage the build next to the bundled assets, like the plugin's notifications/ folder.
$stage = Join-Path $OutDir 'stage'
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item -Recurse -Force "$PSScriptRoot\..\..\..\notifications\assets" $stage
Copy-Item -Force $Exe (Join-Path $stage 'ToastWindow.exe')
$Exe = Join-Path $stage 'ToastWindow.exe'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class W {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    public class T { public long Hwnd; public uint Pid; public int L, Top, R, B; }
    public static List<T> Toasts() {
        var list = new List<T>();
        EnumWindows((h, l) => {
            var sb = new StringBuilder(256);
            GetClassName(h, sb, 256);
            if (sb.ToString() == "ClaudeCodeToast" && IsWindowVisible(h)) {
                RECT r; GetWindowRect(h, out r); uint pid; GetWindowThreadProcessId(h, out pid);
                list.Add(new T { Hwnd = (long)h, Pid = pid, L = r.L, Top = r.T, R = r.R, B = r.B });
            }
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
'@

[W]::SetProcessDPIAware() | Out-Null
function Get-Toasts { [W]::Toasts() }

if ($Mode -eq 'doctor') {
    $item = Get-Item $Exe
    "exe: $Exe ($($item.Length) bytes, $($item.LastWriteTime))"
    "visible toasts: $(@(Get-Toasts).Count)"
    return
}

function Show-Toast([string]$session, [string]$hookMode, [string]$json) {
    $json | & $Exe --save | Out-Null
    $psi = [System.Diagnostics.ProcessStartInfo]::new($Exe)
    $psi.RedirectStandardInput = $true
    $psi.UseShellExecute = $false
    $psi.ArgumentList.Add("--$hookMode")
    $psi.ArgumentList.Add('--debug')
    $p = [System.Diagnostics.Process]::Start($psi)
    $p.StandardInput.Write($json); $p.StandardInput.Close(); $p.WaitForExit()
}

function Snap([string]$name) {
    $ts = @(Get-Toasts)
    if ($ts.Count -eq 0) { throw "no toast visible for $name" }
    $l = ($ts.L | Measure-Object -Minimum).Minimum; $t = ($ts.Top | Measure-Object -Minimum).Minimum
    $r = ($ts.R | Measure-Object -Maximum).Maximum; $b = ($ts.B | Measure-Object -Maximum).Maximum
    if ($r -le $l -or $b -le $t) { throw "bad toast rect $l,$t,$r,$b ($($ts | Out-String))" }
    $bmp = [System.Drawing.Bitmap]::new([int]($r - $l), [int]($b - $t))
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($l, $t, 0, 0, $bmp.Size)
    $path = Join-Path $OutDir "$stamp-$name.png"
    $bmp.Save($path); $g.Dispose(); $bmp.Dispose()
    "screenshot: $path"
    $ts | Format-Table | Out-String
}

$before = @(Get-Toasts).Pid
$sid = "verify-$stamp"
$base = @{ session_id = $sid; cwd = $Cwd; prompt = 'verify prompt line' }
switch ($Mode) {
    'notify' {
        Show-Toast $sid 'notify' ($base | ConvertTo-Json -Compress)
    }
    'input' {
        $j = $base.Clone(); $j.notification_type = 'permission_prompt'
        $j.message = if ($Message) { $Message } else { 'Claude needs your permission to use Bash' }
        if ($Title) { $j.hook_event_name = 'PreToolUse'; $j.tool_name = 'AskUserQuestion'; $j.tool_input = @{ questions = @(@{ question = $j.message }) } }
        Show-Toast $sid 'input' ($j | ConvertTo-Json -Compress -Depth 5)
    }
    'stack' {
        foreach ($i in 1..3) {
            $j = $base.Clone(); $j.session_id = "$sid-$i"; $j.prompt = "stack toast $i"
            Show-Toast $j.session_id 'notify' ($j | ConvertTo-Json -Compress)
            Start-Sleep -Milliseconds 400
        }
    }
}
Start-Sleep -Milliseconds 900
Snap $Mode
if ($Mode -eq 'stack') {
    # Expected: the toast nearest the taskbar (largest Top, oldest) closes first, then the others slide down.
    $alive = @(Get-Toasts | Where-Object { $before -notcontains $_.Pid })
    $bottomPid = ($alive | Sort-Object Top -Descending | Select-Object -First 1).Pid
    $deadline = (Get-Date).AddSeconds(12)
    while ((Get-Date) -lt $deadline) {
        $now = @(Get-Toasts).Pid
        $gone = @($alive | Where-Object { $now -notcontains $_.Pid })
        if ($gone.Count -gt 0) { break }
        Start-Sleep -Milliseconds 100
    }
    $firstGone = $gone | Select-Object -First 1
    if (-not $firstGone) { 'FADE-ORDER FAIL: no toast closed within 12s' }
    elseif ($firstGone.Pid -eq $bottomPid) { "FADE-ORDER PASS: bottom toast (pid $bottomPid) closed first" }
    else { "FADE-ORDER FAIL: pid $($firstGone.Pid) at Top=$($firstGone.Top) closed before bottom pid $bottomPid" }
}
$log = Join-Path (Split-Path $Exe) 'debug.log'
if (Test-Path $log) { Copy-Item $log (Join-Path $OutDir "$stamp-$Mode-debug.log"); "debug log: $OutDir\$stamp-$Mode-debug.log" }

# Cleanup: stop only toast processes this run started, then drop its state files.
Get-Toasts | Where-Object { $before -notcontains $_.Pid } | ForEach-Object { Stop-Process -Id $_.Pid -Force }
Get-ChildItem $env:TEMP -Filter "claude-notify-$sid*.txt" | Remove-Item -Force
"cleanup done"
