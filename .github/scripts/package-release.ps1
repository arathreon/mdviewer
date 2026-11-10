<#
.SYNOPSIS
Packs the release build into a zip file, and makes a SHA-256 checksum file for it.

.DESCRIPTION
The zip file "mdviewer-<version>-windows-x64.zip" has one folder of the same name with:
  - mdviewer.exe (from target/release; run `cargo build --release --locked` first),
  - README.md, and LICENSE.txt (the MIT license of mdviewer),
  - licenses/<library>-<file>.txt: the license files of each browser library that is
    embedded in mdviewer.exe (assets/vendor/<library>/LICENSE, OFL.txt,
    THIRD-PARTY-NOTICES.txt),
  - licenses/rust-crates.txt: the notices of the Rust crates in mdviewer.exe, made now
    from Cargo.lock by tools/notices.mjs (needs Node.js). It stops the script if a
    crate has a license that is not accepted.
The licenses of the libraries and crates require that their notices go with each copy
of the program.

The file "<zip name>.sha256" has the checksum in the format of `sha256sum`
("<hash>  <file name>" and a line feed), so that users can check the download with
`sha256sum -c` or compare it with `Get-FileHash`.

.PARAMETER Version
The version, for example "0.1.0" (from check-release-version.ps1).

.PARAMETER OutDir
The folder for the zip file and the checksum file. The script makes the folder if
necessary, and replaces files of the same name.

.OUTPUTS
The full paths of the zip file and of the checksum file.

.EXAMPLE
./.github/scripts/package-release.ps1 -Version 0.1.0 -OutDir dist
#>
param(
    [Parameter(Mandatory)]
    [string] $Version,
    [string] $OutDir = 'dist'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# The repository root is two folders above this script.
$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$exe = Join-Path $root 'target/release/mdviewer.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    throw "$exe is missing. Run 'cargo build --release --locked' first."
}
# The license files in the top folder of each library (not the library files).
$vendorLicenses = @(Get-ChildItem -File -Path (Join-Path $root 'assets/vendor/*/*') |
    Where-Object { $_.Name -match '^(LICENSE|OFL|THIRD-PARTY-NOTICES)' })
if ($vendorLicenses.Count -eq 0) {
    throw 'assets/vendor has no license files.'
}

$name = "mdviewer-$Version-windows-x64"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path

# A new, empty folder for the zip content, so that no old file goes into the zip.
$stage = Join-Path $OutDir $name
if (Test-Path -LiteralPath $stage) {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
$licenses = New-Item -ItemType Directory -Path (Join-Path $stage 'licenses')

Copy-Item -LiteralPath $exe, (Join-Path $root 'README.md') -Destination $stage
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $stage 'LICENSE.txt')
foreach ($license in $vendorLicenses) {
    # For example assets/vendor/katex/LICENSE -> licenses/katex-LICENSE.txt, and
    # assets/vendor/katex/OFL.txt -> licenses/katex-OFL.txt.
    $target = Join-Path $licenses "$($license.Directory.Name)-$($license.BaseName).txt"
    Copy-Item -LiteralPath $license.FullName -Destination $target
}

# The notices of the Rust crates, from Cargo.lock. The tool writes the file itself, so
# that PowerShell does not decode and change its characters.
$crates = Join-Path $licenses 'rust-crates.txt'
& node (Join-Path $root 'tools/notices.mjs') cargo --out $crates
if ($LASTEXITCODE -ne 0) {
    throw 'tools/notices.mjs found license problems in the Rust crates (see above).'
}

# With a folder as -Path, the zip has that folder at the top, so unpacking makes one
# folder and does not spread the files.
$zip = Join-Path $OutDir "$name.zip"
Compress-Archive -Path $stage -DestinationPath $zip -Force
Remove-Item -LiteralPath $stage -Recurse -Force

# A line feed, not CR LF: `sha256sum -c` would read CR as part of the file name.
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
$checksum = "$zip.sha256"
[System.IO.File]::WriteAllText($checksum, "$hash  $name.zip`n")

Write-Output $zip
Write-Output $checksum
