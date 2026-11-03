<#
.SYNOPSIS
Keeps a portable foobar2000 next to the repository for testing components, so the main
foobar2000 profile is never touched.

.DESCRIPTION
-Setup copies the installed foobar2000 to .fb2k-portable and marks it portable: foobar2000 then
keeps its whole profile in .fb2k-portable\profile.
-Install builds a component in release mode and copies its DLL into the portable profile.
-Run starts the portable foobar2000, passing any further arguments on its command line.

.EXAMPLE
pwsh tools/fb2k-portable.ps1 -Setup
pwsh tools/fb2k-portable.ps1 -Install foo_oxitone
pwsh tools/fb2k-portable.ps1 -Run /add oxitone://440
#>
[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
	[Parameter(ParameterSetName = 'Setup', Mandatory)]
	[switch] $Setup,

	[Parameter(ParameterSetName = 'Setup')]
	[string] $Source = 'C:\Program Files\foobar2000',

	[Parameter(ParameterSetName = 'Install', Mandatory)]
	[string] $Install,

	[Parameter(ParameterSetName = 'Run')]
	[switch] $Run,

	[Parameter(ParameterSetName = 'Run', ValueFromRemainingArguments)]
	[string[]] $Arguments = @()
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$portable = Join-Path $repository '.fb2k-portable'
$executable = Join-Path $portable 'foobar2000.exe'

switch ($PSCmdlet.ParameterSetName) {
	'Setup' {
		if (-not (Test-Path (Join-Path $Source 'foobar2000.exe'))) {
			throw "No foobar2000 at $Source. Pass -Source with the folder of an installed foobar2000 2.x x64."
		}
		if (Test-Path $portable) {
			throw "$portable already exists. Delete it first to start from a fresh profile."
		}
		Copy-Item -Path $Source -Destination $portable -Recurse
		# An empty file with this name makes foobar2000 keep its profile in .\profile.
		New-Item -ItemType File -Path (Join-Path $portable 'portable_mode_enabled') | Out-Null
		Write-Host "Portable foobar2000 ready in $portable"
	}
	'Install' {
		if (-not (Test-Path $executable)) {
			throw "Run -Setup first."
		}
		cargo build --release -p $Install
		if ($LASTEXITCODE -ne 0) {
			throw "cargo build failed."
		}
		$dll = Join-Path $repository "target\release\$Install.dll"
		$destination = Join-Path $portable "profile\user-components-x64\$Install"
		New-Item -ItemType Directory -Force -Path $destination | Out-Null
		Copy-Item -Path $dll -Destination $destination -Force
		Write-Host "Installed $Install into $destination"
	}
	'Run' {
		if (-not (Test-Path $executable)) {
			throw "Run -Setup first."
		}
		if ($Arguments.Count -gt 0) {
			Start-Process -FilePath $executable -ArgumentList $Arguments
		} else {
			Start-Process -FilePath $executable
		}
	}
}
