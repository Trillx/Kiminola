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
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'

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
    $entries[$architecture] = [ordered]@{
        url = $installer.browser_download_url
        signature = Read-Signature -Asset $signatureAsset
    }
}

if ([string]::IsNullOrWhiteSpace($release.body)) {
    $notes = "Kimi Nola $Version"
} else {
    $notes = $release.body.Trim()
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
foreach ($platform in $platforms.Keys) {
    $entry = $parsed.platforms.PSObject.Properties[$platform].Value
    if ($null -eq $entry -or [string]::IsNullOrWhiteSpace($entry.url) -or [string]::IsNullOrWhiteSpace($entry.signature)) {
        throw "Generated updater manifest is missing '$platform'."
    }
}

Write-Host "Generated $OutputPath for $Tag from release $ReleaseId."
