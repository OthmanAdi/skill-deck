[CmdletBinding()]
param(
    [string] $MsiPath,
    [string] $ExpectedProductExe,
    [string] $ExpectedProductDirectory,
    [string] $ExtractRoot,
    [string] $SevenZipPath = '7z',
    [string] $ProvenanceScannerPath = 'scripts/release-compliance/check_binary_provenance.py',
    [string[]] $ProjectRoot = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:MsiPayloadSchema = 'skill-deck-windows-msi-payload/v2'

function Release-ComObject {
    param([AllowNull()][object] $InputObject)
    if ($null -ne $InputObject -and [Runtime.InteropServices.Marshal]::IsComObject($InputObject)) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($InputObject)
    }
}

function Split-MsiCompoundName {
    param([Parameter(Mandatory)][string] $Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { throw 'MSI compound name is empty.' }
    $parts = @($Value.Split('|'))
    $emptyParts = @($parts | Where-Object { [string]::IsNullOrWhiteSpace($_) })
    if ($parts.Count -gt 2 -or $emptyParts.Count -ne 0) {
        throw 'MSI compound name is malformed.'
    }
    return $parts[-1]
}

function Assert-SafeMsiName {
    param(
        [Parameter(Mandatory)][string] $Value,
        [Parameter(Mandatory)][string] $Label
    )
    if (
        [string]::IsNullOrWhiteSpace($Value) -or
        $Value -in '.', '..' -or
        $Value -match '[<>:"/\\|?*\x00-\x1f]'
    ) { throw "$Label is not a safe MSI leaf name." }
}

function Read-WindowsInstallerTable {
    param(
        [Parameter(Mandatory)][object] $Database,
        [Parameter(Mandatory)][string] $Table,
        [Parameter(Mandatory)][string[]] $Columns
    )
    $view = $null
    try {
        $query = 'SELECT ' + (($Columns | ForEach-Object { "``$_``" }) -join ', ') + " FROM ``$Table``"
        $view = $Database.OpenView($query)
        [void] $view.Execute()
        $rows = @()
        while ($null -ne ($record = $view.Fetch())) {
            try {
                $row = [ordered]@{}
                for ($index = 0; $index -lt $Columns.Count; $index++) {
                    $row[$Columns[$index]] = $record.StringData($index + 1)
                }
                $rows += [pscustomobject]$row
            }
            finally { Release-ComObject $record }
        }
        return $rows
    }
    finally {
        if ($null -ne $view) { [void] $view.Close(); Release-ComObject $view }
    }
}

function Read-WindowsInstallerTables {
    param([Parameter(Mandatory)][string] $LiteralMsiPath)
    $installer = $null
    $database = $null
    try {
        $installer = New-Object -ComObject WindowsInstaller.Installer
        $database = $installer.OpenDatabase((Resolve-Path -LiteralPath $LiteralMsiPath).Path, 0)
        return [pscustomobject]@{
            File = @(Read-WindowsInstallerTable $database 'File' @('File', 'Component_', 'FileName', 'FileSize'))
            Component = @(Read-WindowsInstallerTable $database 'Component' @('Component', 'Directory_'))
            Directory = @(Read-WindowsInstallerTable $database 'Directory' @('Directory', 'Directory_Parent', 'DefaultDir'))
        }
    }
    finally {
        Release-ComObject $database
        Release-ComObject $installer
    }
}

function Test-WindowsMsiMetadata {
    param(
        [Parameter(Mandatory)][object[]] $FileRows,
        [Parameter(Mandatory)][object[]] $ComponentRows,
        [Parameter(Mandatory)][object[]] $DirectoryRows,
        [Parameter(Mandatory)][string] $ProductExe,
        [Parameter(Mandatory)][string] $ProductDirectory
    )
    Assert-SafeMsiName $ProductExe 'Expected product executable'
    Assert-SafeMsiName $ProductDirectory 'Expected product directory'
    $normalized = @($FileRows | ForEach-Object {
        $logical = Split-MsiCompoundName ([string]$_.FileName)
        Assert-SafeMsiName $logical 'MSI File table name'
        [pscustomobject]@{
            File = [string]$_.File
            Component = [string]$_.Component_
            LogicalName = $logical
            Size = [long]$_.FileSize
        }
    })
    $forbidden = @($normalized | Where-Object {
        $_.LogicalName -match '(?i)^(license[-_]keygen|harness[-_]cli)\.exe$' -or
        $_.LogicalName -match '(?i)\.pdb$'
    })
    if ($forbidden.Count -ne 0) { throw 'MSI contains a developer-only executable or PDB.' }
    $products = @($normalized | Where-Object { $_.LogicalName -ceq $ProductExe })
    if ($products.Count -ne 1) { throw 'MSI must contain exactly one expected product executable.' }
    $product = $products[0]
    $components = @($ComponentRows | Where-Object { [string]$_.Component -ceq $product.Component })
    if ($components.Count -ne 1) { throw 'Product component mapping is missing or ambiguous.' }
    $directoryKey = [string]$components[0].Directory_
    $visited = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $leaf = $null
    while ($directoryKey -cne 'ProgramFiles64Folder') {
        if (-not $visited.Add($directoryKey)) { throw 'MSI directory chain contains a cycle.' }
        $rows = @($DirectoryRows | Where-Object { [string]$_.Directory -ceq $directoryKey })
        if ($rows.Count -ne 1) { throw 'MSI directory chain is missing or ambiguous.' }
        $name = Split-MsiCompoundName ([string]$rows[0].DefaultDir)
        if ($null -eq $leaf) { $leaf = $name }
        $directoryKey = [string]$rows[0].Directory_Parent
    }
    if ($leaf -cne $ProductDirectory) { throw 'Product executable is outside the expected Program Files directory.' }
    return [pscustomobject]@{
        ProductFileKey = $product.File
        ProductLogicalName = $product.LogicalName
        ProductSize = $product.Size
        ProductDirectory = $leaf
        FileRowCount = $normalized.Count
    }
}

function Get-ExtractedProductPayload {
    param(
        [Parameter(Mandatory)][string] $LiteralRoot,
        [Parameter(Mandatory)][object] $Metadata
    )
    $matches = @(Get-ChildItem -LiteralPath $LiteralRoot -Recurse -File | Where-Object {
        $_.Name -ieq [string]$Metadata.ProductFileKey -or
        $_.Name -ieq [string]$Metadata.ProductLogicalName
    })
    if ($matches.Count -ne 1) { throw 'Extracted MSI must contain exactly one product payload.' }
    $payload = $matches[0]
    if ($payload.Length -ne [long]$Metadata.ProductSize) { throw 'Extracted product size differs from the MSI File table.' }
    $stream = [IO.File]::OpenRead($payload.FullName)
    try {
        if ($stream.ReadByte() -ne 0x4d -or $stream.ReadByte() -ne 0x5a) {
            throw 'Extracted product payload does not have an MZ header.'
        }
    }
    finally { $stream.Dispose() }
    return $payload
}

function Invoke-WindowsMsiPayloadCheck {
    param(
        [Parameter(Mandatory)][string] $LiteralMsiPath,
        [Parameter(Mandatory)][string] $ProductExe,
        [Parameter(Mandatory)][string] $ProductDirectory,
        [Parameter(Mandatory)][string] $LiteralExtractRoot,
        [Parameter(Mandatory)][string] $LiteralSevenZipPath,
        [Parameter(Mandatory)][string] $LiteralProvenanceScannerPath,
        [Parameter(Mandatory)][string[]] $LiteralProjectRoots
    )
    if ($LiteralProjectRoots.Count -eq 0) { throw 'At least one provenance project root is required.' }
    if (Test-Path -LiteralPath $LiteralExtractRoot) { throw 'MSI extraction destination already exists.' }
    $tables = Read-WindowsInstallerTables $LiteralMsiPath
    $metadata = Test-WindowsMsiMetadata $tables.File $tables.Component $tables.Directory $ProductExe $ProductDirectory
    New-Item -ItemType Directory -Path $LiteralExtractRoot | Out-Null
    & $LiteralSevenZipPath x -y -bd "-o$LiteralExtractRoot" -- $LiteralMsiPath 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw '7-Zip could not extract the MSI cabinet payload.' }
    $payload = Get-ExtractedProductPayload $LiteralExtractRoot $metadata
    $arguments = @('-B', $LiteralProvenanceScannerPath, '--json')
    foreach ($root in $LiteralProjectRoots) { $arguments += @('--project-root', $root) }
    $arguments += $payload.FullName
    $scanner = @(& python @arguments) -join [Environment]::NewLine
    if ($LASTEXITCODE -ne 0) { throw 'Extracted MSI application failed provenance validation.' }
    $scannerReport = $scanner | ConvertFrom-Json
    if ($scannerReport.passed -ne $true) { throw 'Provenance scanner did not attest the extracted MSI payload.' }
    return [ordered]@{
        schema = $script:MsiPayloadSchema
        passed = $true
        product = [ordered]@{
            logicalName = $metadata.ProductLogicalName
            fileKey = $metadata.ProductFileKey
            size = $metadata.ProductSize
            installDirectory = 'ProgramFiles64Folder/' + $metadata.ProductDirectory
        }
        fileRowCount = $metadata.FileRowCount
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    try {
        foreach ($required in @($MsiPath, $ExpectedProductExe, $ExpectedProductDirectory, $ExtractRoot, $ProvenanceScannerPath)) {
            if ([string]::IsNullOrWhiteSpace([string]$required)) { throw 'Required MSI payload argument is absent.' }
        }
        Invoke-WindowsMsiPayloadCheck $MsiPath $ExpectedProductExe $ExpectedProductDirectory $ExtractRoot $SevenZipPath $ProvenanceScannerPath $ProjectRoot |
            ConvertTo-Json -Depth 6 -Compress
        exit 0
    }
    catch {
        [ordered]@{
            schema = $script:MsiPayloadSchema
            passed = $false
            findings = @([ordered]@{ category = 'msi-payload-validation-error'; count = 1 })
        } | ConvertTo-Json -Depth 4 -Compress
        exit 1
    }
}
