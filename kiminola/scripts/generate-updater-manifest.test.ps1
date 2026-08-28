# Deterministic offline regression harness for generate-updater-manifest.ps1.
#
# Covers GitHub issue #9: assets on a draft release expose browser_download_url
# values under releases/download/untagged-*, which return 404 once the release
# is published. The updater manifest a published client consumes must use
# permanent tag-versioned installer URLs, retain literal signatures, and carry
# notes that do not claim the release remains a draft.
#
# No network access and no signing secrets are required. Exits 0 when every
# check passes and 1 otherwise.

$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$generator = Join-Path $scriptDir 'generate-updater-manifest.ps1'

$script:failures = @()
function Check([string]$Name, [bool]$Condition) {
    if ($Condition) {
        Write-Host "PASS: $Name"
    } else {
        $script:failures += $Name
        Write-Host "FAIL: $Name"
    }
}

$fixtureRoot = Join-Path $env:TEMP ("kiminola-updater-manifest-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $fixtureRoot | Out-Null

try {
    $repository = 'Trillx/Kiminola'
    $version = '9.9.9'
    $tag = "v$version"
    $releaseId = 424242

    $signatureX64 = 'untrusted comment: signature from tauri secret key fixture-x64'
    $signatureArm64 = 'untrusted comment: signature from tauri secret key fixture-arm64'
    $signatureX64Path = Join-Path $fixtureRoot 'fixture-x64.sig'
    $signatureArm64Path = Join-Path $fixtureRoot 'fixture-arm64.sig'
    [System.IO.File]::WriteAllText($signatureX64Path, $signatureX64)
    [System.IO.File]::WriteAllText($signatureArm64Path, $signatureArm64)

    function New-DraftReleaseFixture([string]$Body, [string]$Path) {
        # Mirrors the GitHub API response for a draft release: draft assets
        # always carry untagged-* browser_download_url values.
        $untagged = "https://github.com/$repository/releases/download/untagged-deadbeef"
        $assets = @(
            [ordered]@{ name = "Kimi.Nola_$($version)_x64-setup.exe"; url = 'https://api.github.com/assets/1'; browser_download_url = "$untagged/Kimi.Nola_$($version)_x64-setup.exe" }
            [ordered]@{ name = "Kimi.Nola_$($version)_x64-setup.exe.sig"; url = 'https://api.github.com/assets/2'; browser_download_url = "$untagged/Kimi.Nola_$($version)_x64-setup.exe.sig"; local_path = $signatureX64Path }
            [ordered]@{ name = "Kimi.Nola_$($version)_arm64-setup.exe"; url = 'https://api.github.com/assets/3'; browser_download_url = "$untagged/Kimi.Nola_$($version)_arm64-setup.exe" }
            [ordered]@{ name = "Kimi.Nola_$($version)_arm64-setup.exe.sig"; url = 'https://api.github.com/assets/4'; browser_download_url = "$untagged/Kimi.Nola_$($version)_arm64-setup.exe.sig"; local_path = $signatureArm64Path }
        )
        $fixture = [ordered]@{
            id = $releaseId
            tag_name = $tag
            draft = $true
            prerelease = $false
            created_at = '2026-01-01T00:00:00Z'
            body = $Body
            assets = $assets
        }
        [System.IO.File]::WriteAllText($Path, ($fixture | ConvertTo-Json -Depth 6))
    }

    # --- Scenario 1: a well-formed draft release produces a publish-safe manifest ---

    $fixturePath = Join-Path $fixtureRoot 'release.json'
    New-DraftReleaseFixture -Body "Windows x64 and ARM64 installers for Kimi Nola $tag." -Path $fixturePath
    $outputPath = Join-Path $fixtureRoot 'latest.json'

    $runError = $null
    try {
        & $generator -Repository $repository -Tag $tag -ReleaseId $releaseId -Version $version -OutputPath $outputPath -ReleaseFixturePath $fixturePath
    } catch {
        $runError = $_
    }
    Check 'generator accepts a valid draft release fixture' ($null -eq $runError)

    if ($null -eq $runError) {
        $manifest = Get-Content -Raw $outputPath | ConvertFrom-Json
        $expectedPrefix = "https://github.com/$repository/releases/download/$tag/"

        foreach ($platform in @('windows-x86_64', 'windows-aarch64', 'windows-x86_64-nsis', 'windows-aarch64-nsis')) {
            $entry = $manifest.platforms.PSObject.Properties[$platform].Value
            Check "$platform entry is present" ($null -ne $entry)
            if ($null -ne $entry) {
                Check "$platform URL is permanent and tag-versioned" ($entry.url.StartsWith($expectedPrefix))
                Check "$platform URL is not a draft-only untagged URL" ($entry.url -notmatch 'untagged')
            }
        }

        Check 'x64 URL names the x64 installer' ($manifest.platforms.'windows-x86_64'.url -eq "$($expectedPrefix)Kimi.Nola_$($version)_x64-setup.exe")
        Check 'arm64 URL names the arm64 installer' ($manifest.platforms.'windows-aarch64'.url -eq "$($expectedPrefix)Kimi.Nola_$($version)_arm64-setup.exe")
        Check 'x64 signature is the literal signature content' ($manifest.platforms.'windows-x86_64'.signature -eq $signatureX64)
        Check 'arm64 signature is the literal signature content' ($manifest.platforms.'windows-aarch64'.signature -eq $signatureArm64)
        Check 'manifest contains no untagged URL anywhere' ((Get-Content -Raw $outputPath) -notmatch 'untagged')
        Check 'version round-trips' ($manifest.version -eq $version)
        Check 'notes come from the release body' ($manifest.notes -eq "Windows x64 and ARM64 installers for Kimi Nola $tag.")
    }

    # --- Scenario 2: notes claiming the release remains a draft are rejected ---

    $staleFixturePath = Join-Path $fixtureRoot 'release-stale-notes.json'
    New-DraftReleaseFixture -Body "Installers for Kimi Nola $tag. This release remains a draft until the update path has been tested." -Path $staleFixturePath
    $staleOutputPath = Join-Path $fixtureRoot 'latest-stale.json'

    $staleError = $null
    try {
        & $generator -Repository $repository -Tag $tag -ReleaseId $releaseId -Version $version -OutputPath $staleOutputPath -ReleaseFixturePath $staleFixturePath
    } catch {
        $staleError = $_
    }
    Check 'generator rejects notes claiming the release remains a draft' ($null -ne $staleError -and "$staleError" -match 'remains a draft')
} finally {
    Remove-Item -Recurse -Force $fixtureRoot -ErrorAction SilentlyContinue
}

if ($script:failures.Count -gt 0) {
    Write-Host ""
    Write-Host "$($script:failures.Count) updater manifest check(s) FAILED."
    exit 1
}
Write-Host "All updater manifest checks passed."
exit 0
