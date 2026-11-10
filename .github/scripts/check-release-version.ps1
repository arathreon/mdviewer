<#
.SYNOPSIS
Checks that a release tag names the version in Cargo.toml, and gives the release version.

.DESCRIPTION
The release workflow (.github/workflows/release.yml) runs this script first, so that a
release "v0.2.0" cannot contain a program of version 0.1.0. Letter case counts, and a
"v" in front of the tag is optional.

- A release must have the tag of the version in Cargo.toml, for example "v0.1.0".
- A pre-release can also add a suffix to the version, for example "v0.1.0-rc.1", then
  "v0.1.0-rc.2". Thus pre-releases need no change of Cargo.toml. The suffix uses the
  rule of semantic versioning: parts of letters, digits and "-", separated by dots.
  These characters are also safe in file names.

The script writes the release version (the tag without "v", for example "0.1.0-rc.1")
to the standard output. In GitHub Actions, it also writes "version=<release version>" to
the file in GITHUB_OUTPUT, so that later jobs can use it (the name of the zip file).

.PARAMETER Tag
The tag of the release, for example "v0.1.0".

.PARAMETER Prerelease
The release is a pre-release ("Set as a pre-release" on GitHub). Only then can the tag
have a suffix.

.PARAMETER Version
The version to compare with. Default: the version in Cargo.toml. The tests give it, so
that they need no cargo.

.EXAMPLE
./.github/scripts/check-release-version.ps1 -Tag v0.1.0

.EXAMPLE
./.github/scripts/check-release-version.ps1 -Tag v0.1.0-rc.1 -Prerelease
#>
param(
    [Parameter(Mandatory)]
    [string] $Tag,

    [switch] $Prerelease,

    [string] $Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $Version) {
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
    $Version = $package.version
}

# The tag without the optional "v" in front.
$release = if ($Tag.StartsWith('v')) { $Tag.Substring(1) } else { $Tag }

# -cmatch: letter case counts. The suffix: semantic versioning pre-release identifiers.
$withSuffix = '^' + [regex]::Escape($Version) + '-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*$'

if ($release -ceq $Version) {
    # A release or a pre-release of exactly the version: always correct.
} elseif ($Prerelease -and $release -cmatch $withSuffix) {
    # A pre-release such as "v0.1.0-rc.1" for the version 0.1.0.
} else {
    # The "::error::" prefix makes GitHub show the message in the summary of the run.
    if ($release -cmatch $withSuffix) {
        $message = "The release tag '$Tag' has a suffix, but the release is not a pre-release. " +
            "Select 'Set as a pre-release', or use the tag 'v$Version'."
    } else {
        $message = "The release tag '$Tag' does not match the version '$Version' in Cargo.toml. " +
            "Use the tag 'v$Version' (a pre-release can also use 'v$Version-rc.1'), or change " +
            'the version in Cargo.toml.'
    }
    Write-Output "::error::$message"
    exit 1
}

if ($env:GITHUB_OUTPUT) {
    # PowerShell 7 writes UTF-8 without a byte order mark, as GitHub expects.
    "version=$release" | Out-File -LiteralPath $env:GITHUB_OUTPUT -Append -Encoding utf8
}
Write-Output $release
