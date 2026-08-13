#requires -Version 7.2

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Destination,

    [switch]$ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDirectory = Split-Path -Parent $MyInvocation.MyCommand.Path
$sourceRoot = [System.IO.Path]::GetFullPath((Join-Path $scriptDirectory '../..')).TrimEnd('\', '/')
$manifestPath = Join-Path $scriptDirectory 'public-export.manifest.json'
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json

if ($manifest.schemaVersion -cne 'skill-deck-public-export/v1') {
    throw "Unsupported public-export manifest: $($manifest.schemaVersion)"
}

function Get-NormalizedRelativePath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,

        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    return [System.IO.Path]::GetRelativePath($Root, $Path).Replace('\', '/')
}

function Test-PathWithin {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Candidate,

        [Parameter(Mandatory = $true)]
        [string]$Parent
    )

    $normalizedCandidate = [System.IO.Path]::GetFullPath($Candidate).TrimEnd('\', '/')
    $normalizedParent = [System.IO.Path]::GetFullPath($Parent).TrimEnd('\', '/')
    if ($normalizedCandidate.Equals($normalizedParent, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $true
    }

    $parentPrefix = $normalizedParent + [System.IO.Path]::DirectorySeparatorChar
    return $normalizedCandidate.StartsWith($parentPrefix, [System.StringComparison]::OrdinalIgnoreCase)
}

function Test-ExcludedPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RelativePath
    )

    foreach ($excludedPath in $manifest.excludedPaths) {
        if ($RelativePath.Equals([string]$excludedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

function Test-ForbiddenDirectoryPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RelativePath
    )

    $parts = @($RelativePath -split '/')
    if ($parts.Count -lt 2) {
        return $false
    }
    foreach ($part in $parts[0..($parts.Count - 2)]) {
        if ($manifest.forbiddenDirectoryNames -icontains $part) {
            return $true
        }
    }
    return $false
}

function Assert-SafeRelativePath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RelativePath
    )

    if ([System.IO.Path]::IsPathRooted($RelativePath)) {
        throw "Manifest path must be relative: $RelativePath"
    }
    if (($RelativePath -eq '..') -or $RelativePath.StartsWith('../', [System.StringComparison]::Ordinal)) {
        throw "Manifest path escapes the source root: $RelativePath"
    }

    if (Test-ForbiddenDirectoryPath -RelativePath $RelativePath) {
        throw "Forbidden directory in public export: $RelativePath"
    }

    $fileName = [System.IO.Path]::GetFileName($RelativePath)
    if ($manifest.forbiddenFileNames -icontains $fileName) {
        throw "Forbidden file in public export: $RelativePath"
    }
    if ($fileName.StartsWith('.env.', [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Environment file is forbidden in public export: $RelativePath"
    }

    $extension = [System.IO.Path]::GetExtension($fileName)
    if ($manifest.forbiddenExtensions -icontains $extension) {
        throw "Generated or binary artifact is forbidden in public export: $RelativePath"
    }
}

function Assert-RegularFile {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.FileInfo]$File,

        [Parameter(Mandatory = $true)]
        [string]$RelativePath
    )

    if (($File.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Reparse points are forbidden in public export: $RelativePath"
    }
}

function Get-ExpectedFiles {
    $expected = [System.Collections.Generic.Dictionary[string, string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )

    foreach ($manifestFile in $manifest.files) {
        $relativePath = ([string]$manifestFile).Replace('\', '/')
        Assert-SafeRelativePath -RelativePath $relativePath
        $fullPath = [System.IO.Path]::GetFullPath((Join-Path $sourceRoot $relativePath))
        if (-not (Test-PathWithin -Candidate $fullPath -Parent $sourceRoot)) {
            throw "Manifest file escapes the source root: $relativePath"
        }
        $file = Get-Item -LiteralPath $fullPath -Force
        if (-not $file.PSIsContainer) {
            Assert-RegularFile -File $file -RelativePath $relativePath
            $expected.Add($relativePath, $file.FullName)
        }
        else {
            throw "Manifest file is a directory: $relativePath"
        }
    }

    foreach ($manifestDirectory in $manifest.directories) {
        $relativeDirectory = ([string]$manifestDirectory).Replace('\', '/').TrimEnd('/')
        Assert-SafeRelativePath -RelativePath "$relativeDirectory/placeholder"
        $fullDirectory = [System.IO.Path]::GetFullPath((Join-Path $sourceRoot $relativeDirectory))
        if (-not (Test-PathWithin -Candidate $fullDirectory -Parent $sourceRoot)) {
            throw "Manifest directory escapes the source root: $relativeDirectory"
        }
        $directory = Get-Item -LiteralPath $fullDirectory -Force
        if (-not $directory.PSIsContainer) {
            throw "Manifest directory is not a directory: $relativeDirectory"
        }
        if (($directory.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Reparse points are forbidden in public export: $relativeDirectory"
        }

        foreach ($file in Get-ChildItem -LiteralPath $fullDirectory -Recurse -File -Force) {
            $relativePath = Get-NormalizedRelativePath -Root $sourceRoot -Path $file.FullName
            if ((Test-ExcludedPath -RelativePath $relativePath) -or
                (Test-ForbiddenDirectoryPath -RelativePath $relativePath)) {
                continue
            }
            Assert-SafeRelativePath -RelativePath $relativePath
            Assert-RegularFile -File $file -RelativePath $relativePath
            if (-not $expected.ContainsKey($relativePath)) {
                $expected.Add($relativePath, $file.FullName)
            }
        }
    }

    return $expected
}

function Test-IsTextFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RelativePath
    )

    $fileName = [System.IO.Path]::GetFileName($RelativePath)
    if ($manifest.extensionlessTextFiles -icontains $fileName) {
        return $true
    }
    return $manifest.textExtensions -icontains [System.IO.Path]::GetExtension($fileName)
}

function Assert-SafeContent {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,

        [Parameter(Mandatory = $true)]
        [string]$RelativePath
    )

    if ($manifest.contentScanExclusions -icontains $RelativePath) {
        return
    }
    if (-not (Test-IsTextFile -RelativePath $RelativePath)) {
        return
    }

    $fullPath = Join-Path $Root $RelativePath
    $content = Get-Content -Raw -LiteralPath $fullPath
    foreach ($rule in $manifest.forbiddenContentPatterns) {
        if ([regex]::IsMatch($content, [string]$rule.pattern, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)) {
            throw "Forbidden content in ${RelativePath}: $($rule.description)"
        }
    }
}

function Assert-ValidDestination {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,

        [Parameter(Mandatory = $true)]
        [System.Collections.Generic.Dictionary[string, string]]$Expected
    )

    if (-not (Test-Path -LiteralPath $Root -PathType Container)) {
        throw "Destination does not exist: $Root"
    }

    $actual = [System.Collections.Generic.Dictionary[string, string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    foreach ($file in Get-ChildItem -LiteralPath $Root -Recurse -File -Force) {
        $relativePath = Get-NormalizedRelativePath -Root $Root -Path $file.FullName
        Assert-SafeRelativePath -RelativePath $relativePath
        Assert-RegularFile -File $file -RelativePath $relativePath
        $actual.Add($relativePath, $file.FullName)
    }

    $missing = @($Expected.Keys | Where-Object { -not $actual.ContainsKey($_) })
    $extra = @($actual.Keys | Where-Object { -not $Expected.ContainsKey($_) })
    if (($missing.Count -gt 0) -or ($extra.Count -gt 0)) {
        $summary = "missing=$($missing -join ', '); extra=$($extra -join ', ')"
        throw "Destination file set does not match the allowlist: $summary"
    }

    foreach ($relativePath in $Expected.Keys) {
        $sourceHash = (Get-FileHash -LiteralPath $Expected[$relativePath] -Algorithm SHA256).Hash
        $destinationHash = (Get-FileHash -LiteralPath $actual[$relativePath] -Algorithm SHA256).Hash
        if ($sourceHash -cne $destinationHash) {
            throw "Destination file differs from source: $relativePath"
        }
        Assert-SafeContent -Root $Root -RelativePath $relativePath
    }

    Write-Output "[OK] Public export validated: $($Expected.Count) files in $Root"
}

$destinationRoot = [System.IO.Path]::GetFullPath($Destination).TrimEnd('\', '/')
if ((Test-PathWithin -Candidate $destinationRoot -Parent $sourceRoot) -or
    (Test-PathWithin -Candidate $sourceRoot -Parent $destinationRoot)) {
    throw 'Destination and source must be separate, non-nested directories.'
}

$expectedFiles = Get-ExpectedFiles

if (-not $ValidateOnly) {
    if (Test-Path -LiteralPath $destinationRoot) {
        $existing = @(Get-ChildItem -LiteralPath $destinationRoot -Force)
        if ($existing.Count -gt 0) {
            throw 'Destination must be absent or empty. The export never overwrites or deletes existing content.'
        }
    }
    else {
        New-Item -ItemType Directory -Path $destinationRoot | Out-Null
    }

    foreach ($relativePath in $expectedFiles.Keys) {
        $destinationFile = Join-Path $destinationRoot $relativePath
        $destinationDirectory = Split-Path -Parent $destinationFile
        if (-not (Test-Path -LiteralPath $destinationDirectory)) {
            New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
        }
        Copy-Item -LiteralPath $expectedFiles[$relativePath] -Destination $destinationFile
    }
}

Assert-ValidDestination -Root $destinationRoot -Expected $expectedFiles
