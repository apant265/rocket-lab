param(
    [string]$Mission = 'examples/plumbbob-provisional.json',
    [string]$Design = '',
    [string]$Out = 'assets/models',
    [string]$Blender = ''
)
$ErrorActionPreference = 'Stop'
$projectPath = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectPath
try {
    if (-not $Blender) {
        $blenderCommand = Get-Command blender -ErrorAction SilentlyContinue
        if ($blenderCommand) { $Blender = $blenderCommand.Source }
        else {
            $foundationPath = Join-Path $env:ProgramFiles 'Blender Foundation'
            $candidate = Get-ChildItem -LiteralPath $foundationPath -Directory | Sort-Object LastWriteTime -Descending |
                ForEach-Object { Join-Path $_.FullName 'blender.exe' } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
            if (-not $candidate) { throw 'Blender not found. Pass -Blender with its executable path.' }
            $Blender = $candidate
        }
    }
    $resolvedMission = (Resolve-Path -LiteralPath $Mission).Path
    if (-not $Design -and $resolvedMission -eq (Join-Path $projectPath 'examples\plumbbob-provisional.json')) {
        $Design = 'sources/Plumbbob_MKIII_R75.CDX1'
    }
    $blenderArgs = @('--background','--factory-startup','--python',(Join-Path $PSScriptRoot 'build_model.py'),'--','--mission',$resolvedMission,'--out',$Out)
    if ($Design) { $blenderArgs += @('--design',(Resolve-Path -LiteralPath $Design).Path) }
    & $Blender @blenderArgs
    if ($LASTEXITCODE -ne 0) { throw "Headless Blender failed with code $LASTEXITCODE" }
} finally { Pop-Location }
