<#
.SYNOPSIS
    Builds ClipFrame's Velopack installer (Setup.exe), delta packages and release feed.

.DESCRIPTION
    Needs the vpk tool:  dotnet tool install -g vpk   (requires the .NET 8 SDK).
    Output goes to installer\Releases. Publishing to GitHub is a separate, explicit step:
        vpk upload github --repoUrl https://github.com/<owner>/<repo> --publish --releaseName "ClipFrame <version>" --tag v<version> --token <token>
    (vpk download github first, so the new release gets delta updates from the previous one.)

.EXAMPLE
    .\installer\package.ps1
    .\installer\package.ps1 -Version 0.2.0
#>
param(
    # Defaults to the workspace version in Cargo.toml.
    [string]$Version,
    [string]$Channel = 'win'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if (-not $Version) {
    $Version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
}
$vpk = Get-Command vpk -ErrorAction SilentlyContinue
if (-not $vpk) { $vpk = Get-Command "$env:USERPROFILE\.dotnet\tools\vpk.exe" -ErrorAction SilentlyContinue }
if (-not $vpk) { throw 'vpk not found. Install it with: dotnet tool install -g vpk' }

Write-Host "Building ClipFrame $Version (release)..."
cargo build --release -p cf-app
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$pack = Join-Path $root 'target\pack'
Remove-Item $pack -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $pack | Out-Null
Copy-Item target\release\clipframe.exe $pack

Write-Host "Packing with Velopack..."
# The pack id names the install folder (%LocalAppData%\<packId>), which uninstall deletes.
# It must differ from "ClipFrame": %LocalAppData%\ClipFrame holds the user's logs, history
# and state, which must survive reinstalls.
& $vpk.Source pack `
    --packId ClipFrameApp `
    --packVersion $Version `
    --packDir $pack `
    --mainExe clipframe.exe `
    --packTitle ClipFrame `
    --packAuthors ClipFrame `
    --icon crates\cf-app\assets\clipframe.ico `
    --channel $Channel `
    --outputDir installer\Releases
if ($LASTEXITCODE -ne 0) { throw 'vpk pack failed' }
Get-ChildItem installer\Releases | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } } | Format-Table -AutoSize
