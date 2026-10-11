param([switch]$Sounding, [switch]$Hpr)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$missionPath = if ($Sounding) { 'examples/sounding.json' } elseif ($Hpr) { 'examples/hpr.json' } else { 'examples/kate-anvil-desert.json' }
& cargo run --locked --features viewer -- view --mission $missionPath --start-time 0
if ($LASTEXITCODE -ne 0) { throw "Rocket Lab exited with code $LASTEXITCODE" }
