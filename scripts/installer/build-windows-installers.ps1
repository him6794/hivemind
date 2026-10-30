param(
    [string]$MasterPackage = "dist\release\windows-master",
    [string]$WorkerPackage = "dist\release\windows-worker",
    [string]$OutputDir = "dist\release",
    [string]$Version = "0.1.0"
)

# Wraps a finished Master or Worker package directory into a per-user Windows installer with NSIS.
# The installer carries exactly the files of the package directory, so it cannot add anything the
# package build did not produce. Nothing is signed here; see the release notes for signing status.

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$makensis = Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"
if (!(Test-Path -LiteralPath $makensis -PathType Leaf)) {
    throw "makensis.exe not found at $makensis. Install NSIS 3.x (winget install NSIS.NSIS)."
}
$template = Join-Path $PSScriptRoot "hivemind-app.nsi"
$icon = Join-Path $repoRoot "hivemind-rs\crates\hivemind-bin\icons\icon.ico"
if (!(Test-Path -LiteralPath $icon -PathType Leaf)) {
    throw "Application icon not found: $icon"
}

$outRoot = Join-Path $repoRoot $OutputDir
$work = Join-Path $repoRoot "dist\installer-work"
New-Item -ItemType Directory -Force -Path $outRoot, $work | Out-Null
$version4 = "$Version.0"

function New-PackageIncludes {
    param([string]$PackagePath, [string]$Id)

    $files = @(Get-ChildItem -LiteralPath $PackagePath -File -Recurse | Sort-Object FullName)
    $fileLines = New-Object System.Collections.Generic.List[string]
    $uninstallLines = New-Object System.Collections.Generic.List[string]
    $directories = New-Object System.Collections.Generic.HashSet[string]
    $lastDir = $null
    foreach ($file in $files) {
        $relative = $file.FullName.Substring($PackagePath.Length).TrimStart('\')
        $relativeDir = [IO.Path]::GetDirectoryName($relative)
        if ($relativeDir -ne $lastDir) {
            if ([string]::IsNullOrEmpty($relativeDir)) {
                $fileLines.Add('SetOutPath "$INSTDIR"')
            } else {
                $fileLines.Add('SetOutPath "$INSTDIR\' + $relativeDir + '"')
                [void]$directories.Add($relativeDir)
            }
            $lastDir = $relativeDir
        }
        $fileLines.Add('File "' + $file.FullName + '"')
        $uninstallLines.Add('Delete "$INSTDIR\' + $relative + '"')
    }
    # Deepest folders first, so each RMDir sees its children already gone. RMDir without /r removes
    # only empty folders, so anything the user added stays.
    foreach ($dir in ($directories | Sort-Object { ($_ -split '\\').Count } -Descending)) {
        $uninstallLines.Add('RMDir "$INSTDIR\' + $dir + '"')
    }
    $filesNsh = Join-Path $work "$Id-files.nsh"
    $uninstallNsh = Join-Path $work "$Id-uninstall.nsh"
    Set-Content -LiteralPath $filesNsh -Value $fileLines -Encoding ASCII
    Set-Content -LiteralPath $uninstallNsh -Value $uninstallLines -Encoding ASCII
    return [pscustomobject]@{
        FilesNsh = $filesNsh
        UninstallNsh = $uninstallNsh
        FileCount = $files.Count
    }
}

function Build-Installer {
    param(
        [string]$Package,
        [string]$AppName,
        [string]$AppId,
        [string]$AppExe,
        [string]$AppHelperExe
    )

    $packagePath = (Resolve-Path -LiteralPath (Join-Path $repoRoot $Package)).Path.TrimEnd('\')
    foreach ($required in @($AppExe, $AppHelperExe, "SHA256SUMS")) {
        if (!(Test-Path -LiteralPath (Join-Path $packagePath $required) -PathType Leaf)) {
            throw "$required is missing from the package directory $packagePath"
        }
    }
    $includes = New-PackageIncludes -PackagePath $packagePath -Id $AppId
    $outFile = Join-Path $outRoot "Hivemind-$($AppId -replace 'Hivemind', '')-Setup-$Version.exe"

    $defines = @(
        "/DAPP_NAME=$AppName",
        "/DAPP_ID=$AppId",
        "/DAPP_VERSION=$Version",
        "/DAPP_VERSION_4=$version4",
        "/DAPP_EXE=$AppExe",
        "/DAPP_HELPER_EXE=$AppHelperExe",
        "/DOUT_FILE=$outFile",
        "/DICON_FILE=$icon",
        "/DFILES_NSH=$($includes.FilesNsh)",
        "/DUNINSTALL_NSH=$($includes.UninstallNsh)"
    )
    & $makensis /V2 @defines $template
    if ($LASTEXITCODE -ne 0) {
        throw "makensis failed for $AppName with exit code $LASTEXITCODE."
    }
    if (!(Test-Path -LiteralPath $outFile -PathType Leaf)) {
        throw "Installer was not produced: $outFile"
    }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $outFile).Hash.ToLowerInvariant()
    "$hash *$(Split-Path -Leaf $outFile)" | Set-Content -LiteralPath "$outFile.sha256" -Encoding ASCII
    [pscustomobject]@{
        Installer = $outFile
        Bytes = (Get-Item -LiteralPath $outFile).Length
        Sha256 = $hash
        PackagedFiles = $includes.FileCount
    }
}

Build-Installer -Package $MasterPackage -AppName "Hivemind Master" -AppId "HivemindMaster" -AppExe "hivemind-master.exe" -AppHelperExe "hivemind-master-ui.exe"
Build-Installer -Package $WorkerPackage -AppName "Hivemind Worker" -AppId "HivemindWorker" -AppExe "hivemind-worker.exe" -AppHelperExe "hivemind-worker-ui.exe"
