param(
    [Parameter(Mandatory = $true)][int]$ClientProcessId,
    [ValidateSet('state', 'close', 'minimize', 'restore', 'inspect', 'press', 'screenshot')][string]$Action = 'state',
    [ValidateSet('Cancel', 'Quit', 'Keep in background')][string]$Button,
    [string]$Screenshot
)
$ErrorActionPreference = 'Stop'
$client = [System.Diagnostics.Process]::GetProcessById($ClientProcessId)
if ($client.ProcessName -notin @('hivemind-master-ui', 'hivemind-worker-ui')) {
    throw 'This test helper only controls Hivemind UI helper processes.'
}
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class HivemindWindowTest {
    public delegate bool WindowCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(WindowCallback callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr deviceContext, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string className, string title);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }
    public static IntPtr FindClient(int processId) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((window, parameter) => {
            uint id;
            GetWindowThreadProcessId(window, out id);
            var title = new StringBuilder(512);
            GetWindowText(window, title, title.Capacity);
            if (id == processId && title.ToString().StartsWith("Hivemind")) {
                result = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
'@
$window = [HivemindWindowTest]::FindClient($ClientProcessId)
if ($window -eq [IntPtr]::Zero) { throw 'The native client window was not found.' }
$title = New-Object System.Text.StringBuilder 512
[void][HivemindWindowTest]::GetWindowText($window, $title, $title.Capacity)
if ($Action -eq 'close') {
    if (-not [HivemindWindowTest]::PostMessage($window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
        throw 'WM_CLOSE could not be posted.'
    }
}
if ($Action -eq 'minimize') {
    if (-not [HivemindWindowTest]::PostMessage($window, 0x0112, [IntPtr]0xF020, [IntPtr]::Zero)) {
        throw 'The client window could not be minimized.'
    }
}
if ($Action -eq 'restore') {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $shellWindow = [HivemindWindowTest]::FindWindow('Shell_TrayWnd', $null)
    if ($shellWindow -eq [IntPtr]::Zero) { throw 'Windows notification area was not found.' }
    $shell = [System.Windows.Automation.AutomationElement]::FromHandle($shellWindow)
    $condition = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty), $title.ToString()
    function FindClickableTrayIcon($automationRoot) {
        $automationRoot.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition) |
            Where-Object { -not $_.Current.IsOffscreen -and $_.Current.BoundingRectangle.Width -gt 0 -and $_.Current.BoundingRectangle.Height -gt 0 } |
            Select-Object -First 1
    }
    $icon = FindClickableTrayIcon $shell
    if ($null -eq $icon) {
        $overflowCondition = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::AutomationIdProperty), 'SystemTrayIcon'
        $overflow = $shell.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $overflowCondition)
        if ($null -eq $overflow) { throw 'The hidden notification icons button was not found.' }
        $pattern = $overflow.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $pattern.Invoke()
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        do {
            $icon = FindClickableTrayIcon ([System.Windows.Automation.AutomationElement]::RootElement)
            if ($null -eq $icon) { Start-Sleep -Milliseconds 100 }
        } while ($null -eq $icon -and [DateTime]::UtcNow -lt $deadline)
    }
    if ($null -eq $icon) { throw 'The Hivemind tray icon was not found after opening the notification overflow.' }
    $rectangle = $icon.Current.BoundingRectangle
    if ($rectangle.IsEmpty -or $rectangle.Width -le 0) { throw 'The Hivemind tray icon has no clickable bounds.' }
    $previous = New-Object HivemindWindowTest+Point
    [void][HivemindWindowTest]::GetCursorPos([ref]$previous)
    try {
        [void][HivemindWindowTest]::SetCursorPos([int]($rectangle.X + $rectangle.Width / 2), [int]($rectangle.Y + $rectangle.Height / 2))
        [HivemindWindowTest]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [HivemindWindowTest]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    } finally {
        [void][HivemindWindowTest]::SetCursorPos($previous.X, $previous.Y)
    }
}
$state = @{ processId = $ClientProcessId; windowHandle = $window.ToInt64(); title = $title.ToString(); visible = [HivemindWindowTest]::IsWindowVisible($window); minimized = [HivemindWindowTest]::IsIconic($window); foreground = [HivemindWindowTest]::GetForegroundWindow() -eq $window }
if ($Action -in @('inspect', 'press', 'screenshot')) {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($window)
    if ($Action -eq 'inspect') {
        $controls = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
        $state.controls = @($controls | ForEach-Object { @{ name = $_.Current.Name; type = $_.Current.ControlType.ProgrammaticName; offscreen = $_.Current.IsOffscreen } })
    }
    if ($Action -eq 'press') {
        if (-not $Button) { throw 'A lifecycle button is required.' }
        $condition = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty), $Button
        $candidates = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
        $target = $candidates | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and $_.Current.IsEnabled -and -not $_.Current.IsOffscreen } | Select-Object -First 1
        if ($null -eq $target) { throw 'The lifecycle button was not found.' }
        $pattern = $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $pattern.Invoke()
    }
    if ($Action -eq 'screenshot') {
        if (-not $Screenshot -or [System.IO.Path]::GetExtension($Screenshot) -ne '.png') { throw 'A PNG evidence path is required.' }
        Add-Type -AssemblyName System.Drawing
        $bounds = $root.Current.BoundingRectangle
        $bitmap = New-Object System.Drawing.Bitmap ([int]$bounds.Width), ([int]$bounds.Height)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $deviceContext = $graphics.GetHdc()
            try {
                if (-not [HivemindWindowTest]::PrintWindow($window, $deviceContext, 2)) { throw 'The client window could not be captured.' }
            } finally {
                $graphics.ReleaseHdc($deviceContext)
            }
            $bitmap.Save($Screenshot, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $graphics.Dispose()
            $bitmap.Dispose()
        }
    }
}
$state | ConvertTo-Json -Compress -Depth 4
