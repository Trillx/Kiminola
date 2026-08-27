[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Repository,

    [Parameter(Mandatory = $true)]
    [string]$Tag,

    [Parameter(Mandatory = $true)]
    [long]$ReleaseId,

    [Parameter(Mandatory = $true)]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [Parameter(Mandatory = $false)]
    [string]$ReleaseFixturePath
)

$ErrorActionPreference = 'Stop'

# Offline fixture mode keeps the generator deterministic for tests; the
# release workflow always uses the live GitHub API path.
$useFixture = -not [string]::IsNullOrWhiteSpace($ReleaseFixturePath)

if ($useFixture) {
    $release = Get-Content -Raw $ReleaseFixturePath | ConvertFrom-Json
} else {
    if ([string]::IsNullOrWhiteSpace($env:GH_TOKEN)) {
        throw 'GH_TOKEN must be set so the draft release assets can be inspected.'
    }

    $headers = @{
        Authorization = "Bearer $env:GH_TOKEN"
        Accept = 'application/vnd.github+json'
        'X-GitHub-Api-Version' = '2022-11-28'
    }
    $releaseUri = "https://api.github.com/repos/$Repository/releases/$ReleaseId"
    $release = Invoke-RestMethod -Method Get -Uri $releaseUri -Headers $headers
}

if ([long]$release.id -ne $ReleaseId) {
    throw "GitHub returned release '$($release.id)' instead of '$ReleaseId'."
}
if ($release.tag_name -ne $Tag) {
    throw "Release tag '$($release.tag_name)' does not match '$Tag'."
}
if ($Tag -ne "v$Version") {
    throw "Release tag '$Tag' does not match app version '$Version'."
}
if (-not $release.draft -or $release.prerelease) {
    throw "Release '$Tag' must be a draft and must not be a prerelease while it is being validated."
}

$escapedVersion = [regex]::Escape($Version)

function Find-ReleaseAsset {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Architecture,

        [Parameter(Mandatory = $true)]
        [bool]$Signature
    )

    $escapedArchitecture = [regex]::Escape($Architecture)
    $pattern = "^Kimi(?:\.| )Nola_${escapedVersion}_${escapedArchitecture}-setup\.exe"
    $pattern += if ($Signature) { '\.sig$' } else { '$' }
    $matches = @($release.assets | Where-Object { $_.name -match $pattern })
    if ($matches.Count -ne 1) {
        $kind = if ($Signature) { 'signature' } else { 'installer' }
        throw "Expected exactly one $kind asset for '$Architecture'; found $($matches.Count)."
    }
    return $matches[0]
}

function Read-Signature {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Asset
    )

    if ($useFixture -and $Asset.PSObject.Properties['local_path']) {
        # Fixture mode: signature content comes from a local file.
        $signature = [System.IO.File]::ReadAllText($Asset.local_path)
    } else {
        $response = Invoke-RestMethod -Method Get -Uri $Asset.url -Headers (@{
                Authorization = "Bearer $env:GH_TOKEN"
                Accept = 'application/octet-stream'
                'X-GitHub-Api-Version' = '2022-11-28'
            })
        $signature = if ($response -is [byte[]]) {
            [System.Text.Encoding]::UTF8.GetString($response)
        } else {
            [string]$response
        }
    }
    $signature = $signature.Trim()
    if ([string]::IsNullOrWhiteSpace($signature)) {
        throw "Signature asset '$($Asset.name)' is empty."
    }
    return $signature
}

$entries = @{}
foreach ($architecture in @('x64', 'arm64')) {
    $installer = Find-ReleaseAsset -Architecture $architecture -Signature $false
    $signatureAsset = Find-ReleaseAsset -Architecture $architecture -Signature $true
    if ($signatureAsset.name -ne "$($installer.name).sig") {
        throw "Signature '$($signatureAsset.name)' does not match installer '$($installer.name)'."
    }
    # Draft assets expose browser_download_url under releases/download/untagged-*,
    # which stops resolving as soon as the release is published. Build the
    # permanent tag-versioned URL instead; it is stable from draft onward.
    $downloadUrl = "https://github.com/$Repository/releases/download/$Tag/$([uri]::EscapeDataString($installer.name))"
    $entries[$architecture] = [ordered]@{
        url = $downloadUrl
        signature = Read-Signature -Asset $signatureAsset
    }
}

if ([string]::IsNullOrWhiteSpace($release.body)) {
    $notes = "Kimi Nola $Version"
} else {
    $notes = $release.body.Trim()
}
if ($notes -match 'remains a draft') {
    throw "Release notes must not claim the release remains a draft; clients keep showing these notes after publication."
}

$platforms = [ordered]@{
    'windows-x86_64' = $entries['x64']
    'windows-aarch64' = $entries['arm64']
    'windows-x86_64-nsis' = $entries['x64']
    'windows-aarch64-nsis' = $entries['arm64']
}
$manifest = [ordered]@{
    version = $Version
    notes = $notes
    pub_date = $release.created_at
    platforms = $platforms
}

$outputDirectory = Split-Path -Parent $OutputPath
if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
}
$json = $manifest | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($OutputPath, $json, [System.Text.UTF8Encoding]::new($false))

$parsed = Get-Content -Raw $OutputPath | ConvertFrom-Json
if ($parsed.version -ne $Version) {
    throw 'Generated updater manifest did not round-trip with the expected version.'
}
$expectedUrlPrefix = "https://github.com/$Repository/releases/download/$Tag/"
foreach ($platform in $platforms.Keys) {
    $entry = $parsed.platforms.PSObject.Properties[$platform].Value
    if ($null -eq $entry -or [string]::IsNullOrWhiteSpace($entry.url) -or [string]::IsNullOrWhiteSpace($entry.signature)) {
        throw "Generated updater manifest is missing '$platform'."
    }
    if (-not $entry.url.StartsWith($expectedUrlPrefix) -or $entry.url -match 'untagged') {
        throw "Updater manifest URL for '$platform' is not a permanent tag-versioned URL: $($entry.url)"
    }
}

Write-Host "Generated $OutputPath for $Tag from release $ReleaseId."
