$ErrorActionPreference = 'Stop'
$scriptPath = Join-Path (Split-Path $PSScriptRoot -Parent) 'check_windows_msi_payload.ps1'
. $scriptPath

$script:passed = 0
function Invoke-Test {
    param([string] $Name, [scriptblock] $Body)
    & $Body
    $script:passed++
    Write-Output "PASS $Name"
}

function New-Rows {
    param([string] $ProductName = 'skill-deck.exe', [string] $DirectoryName = 'Skill Deck')
    return [pscustomobject]@{
        Files = @(
            [pscustomobject]@{ File = 'skilldeck'; Component_ = 'app'; FileName = "SKILLD~1.EXE|$ProductName"; FileSize = '128' },
            [pscustomobject]@{ File = 'notice'; Component_ = 'legal'; FileName = 'NOTICE~1.TXT|THIRD_PARTY_NOTICES.txt'; FileSize = '64' }
        )
        Components = @(
            [pscustomobject]@{ Component = 'app'; Directory_ = 'APPDIR' },
            [pscustomobject]@{ Component = 'legal'; Directory_ = 'APPDIR' }
        )
        Directories = @(
            [pscustomobject]@{ Directory = 'APPDIR'; Directory_Parent = 'ProgramFiles64Folder'; DefaultDir = "SKILLD~1|$DirectoryName" }
        )
    }
}

Invoke-Test 'accepts one application in the expected Program Files directory' {
    $rows = New-Rows
    $result = Test-WindowsMsiMetadata $rows.Files $rows.Components $rows.Directories 'skill-deck.exe' 'Skill Deck'
    if ($result.ProductFileKey -cne 'skilldeck' -or $result.FileRowCount -ne 2) { throw 'Unexpected product metadata.' }
}

Invoke-Test 'rejects a developer-only executable' {
    $rows = New-Rows
    $rows.Files += [pscustomobject]@{ File = 'tool'; Component_ = 'app'; FileName = 'license-keygen.exe'; FileSize = '2' }
    try {
        Test-WindowsMsiMetadata $rows.Files $rows.Components $rows.Directories 'skill-deck.exe' 'Skill Deck' | Out-Null
        throw 'Developer tool was accepted.'
    }
    catch {
        if ($_.Exception.Message -eq 'Developer tool was accepted.') { throw }
    }
}

Invoke-Test 'rejects PDB payloads' {
    $rows = New-Rows
    $rows.Files += [pscustomobject]@{ File = 'symbols'; Component_ = 'app'; FileName = 'skill-deck.pdb'; FileSize = '2' }
    try {
        Test-WindowsMsiMetadata $rows.Files $rows.Components $rows.Directories 'skill-deck.exe' 'Skill Deck' | Out-Null
        throw 'PDB was accepted.'
    }
    catch {
        if ($_.Exception.Message -eq 'PDB was accepted.') { throw }
    }
}

Invoke-Test 'rejects the wrong install directory' {
    $rows = New-Rows -DirectoryName 'Unexpected'
    try {
        Test-WindowsMsiMetadata $rows.Files $rows.Components $rows.Directories 'skill-deck.exe' 'Skill Deck' | Out-Null
        throw 'Wrong directory was accepted.'
    }
    catch {
        if ($_.Exception.Message -eq 'Wrong directory was accepted.') { throw }
    }
}

Invoke-Test 'rejects duplicate application payloads' {
    $rows = New-Rows
    $rows.Files += [pscustomobject]@{ File = 'duplicate'; Component_ = 'app'; FileName = 'skill-deck.exe'; FileSize = '128' }
    try {
        Test-WindowsMsiMetadata $rows.Files $rows.Components $rows.Directories 'skill-deck.exe' 'Skill Deck' | Out-Null
        throw 'Duplicate application was accepted.'
    }
    catch {
        if ($_.Exception.Message -eq 'Duplicate application was accepted.') { throw }
    }
}

Write-Output "PASS $script:passed MSI payload contract tests"
