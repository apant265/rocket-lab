@echo off
pushd "%~dp0"
if exist "target\debug\rocket-lab.exe" (
  "target\debug\rocket-lab.exe" view --mission "examples\plumbbob-provisional.json"
) else (
  cargo run --locked --features viewer -- view --mission "examples\plumbbob-provisional.json"
)
popd
