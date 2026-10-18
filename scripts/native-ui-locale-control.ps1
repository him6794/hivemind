param(
    [Parameter(Mandatory = $true)][int]$ClientProcessId,
    [Parameter(Mandatory = $true)][long]$WindowHandle,
    [ValidatePattern('^[01](,[01])*$')][string]$Sequence = '0',
    [ValidateSet('select', 'press')][string]$Action = 'select',
    [string]$Button
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$client = [System.Diagnostics.Process]::GetProcessById($ClientProcessId)
if ($client.ProcessName -notin @('hivemind-master-ui', 'hivemind-worker-ui')) {
    throw 'Only the spawned Hivemind UI helper may be controlled.'
}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$WindowHandle)
if ($root.Current.ProcessId -ne $ClientProcessId) { throw 'Window ownership changed.' }
if ($Action -eq 'press') {
    if (-not $Button) { throw 'A button name is required.' }
    $condition = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty), $Button
    $target = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition) | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and $_.Current.IsEnabled -and -not $_.Current.IsOffscreen } | Select-Object -First 1
    if ($null -eq $target) { throw 'The requested button was not found.' }
    $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    @{ invoked = $true; processId = $ClientProcessId } | ConvertTo-Json -Compress
    exit 0
}
foreach ($index in $Sequence.Split(',')) {
    $comboCondition = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ControlTypeProperty), ([System.Windows.Automation.ControlType]::ComboBox)
    $combos = @($root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $comboCondition) | Where-Object { -not $_.Current.IsOffscreen })
    if ($combos.Count -ne 1) { throw 'Expected exactly one visible language selector.' }
    $combo = $combos[0]
    $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $condition = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::IsSelectionItemPatternAvailableProperty), $true
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $items = @($combo.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition))
        if ($items.Count -lt 2) { Start-Sleep -Milliseconds 100 }
    } while ($items.Count -lt 2 -and [DateTime]::UtcNow -lt $deadline)
    if ($items.Count -ne 2 -or $items[0].Current.Name -ne 'English') { throw 'Expected the two fixed locale options.' }
    $items[[int]$index].GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
@{ selectedSequence = $Sequence; processId = $ClientProcessId; windowHandle = $WindowHandle } | ConvertTo-Json -Compress
