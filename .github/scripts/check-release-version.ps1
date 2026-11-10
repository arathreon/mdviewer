<#
.SYNOPSIS
Checks that a release tag names the version in Cargo.toml, and gives the version.

.DESCRIPTION
The release workflow (.github/workflows/release.yml) runs this script first, so that a
release "v0.2.0" cannot contain a program of version 0.1.0. The tag must be the version,
with or without a "v" in front (for example "v0.1.0" or "0.1.0"). Letter case counts.

The script writes the version to the standard output. In GitHub Actions, it also writes
"version=<version>" to the file in GITHUB_OUTPUT, so that later jobs can use it.

.PARAMETER Tag
The tag of the release, for example "v0.1.0".

.EXAMPLE
./.github/scripts/check-release-version.ps1 -Tag v0.1.0
#>
param(
    [Parameter(Mandatory)]
    [string] $Tag
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# `cargo metadata` gives the version as Cargo reads it, so the script does not search
# the text of Cargo.toml. `--no-deps` needs no download.
$json = cargo metadata --no-deps --format-version 1 --manifest-path (Join-Path $PSScriptRoot '../../Cargo.toml')
if ($LASTEXITCODE -ne 0) {
    throw 'cargo metadata failed.'
}
$package = ($json | ConvertFrom-Json).packages | Where-Object { $_.name -eq 'mdviewer' }
if (-not $package) {
    throw 'Cargo.toml has no package "mdviewer".'
}
$version = $package.version

if ($Tag -cne $version -and $Tag -cne "v$version") {
    # The "::error::" prefix makes GitHub show the message in the summary of the run.
    Write-Output ("::error::The release tag '$Tag' does not match the version '$version' " +
        "in Cargo.toml. Use the tag 'v$version', or change the version in Cargo.toml.")
    exit 1
}

if ($env:GITHUB_OUTPUT) {
    # PowerShell 7 writes UTF-8 without a byte order mark, as GitHub expects.
    "version=$version" | Out-File -LiteralPath $env:GITHUB_OUTPUT -Append -Encoding utf8
}
Write-Output $version
