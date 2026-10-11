param([string]$Name = ('rocket-lab-windows-' + (Get-Date -Format 'yyyyMMdd-HHmmss')))
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
if ($Name -notmatch '^rocket-lab-windows-[a-zA-Z0-9-]+$') { throw 'Use a rocket-lab-windows- name containing only letters, numbers and hyphens.' }
$packageRoot = Join-Path $projectRoot 'output/package'
New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
$bundle = Join-Path $packageRoot $Name
if (Test-Path -LiteralPath $bundle) { throw 'Choose a fresh package name; existing bundles are preserved.' }
New-Item -ItemType Directory -Path $bundle | Out-Null
$exe = Join-Path $projectRoot 'target/debug/rocket-lab.exe'
if (!(Test-Path -LiteralPath $exe)) { throw 'First build with cargo build --features viewer.' }
Copy-Item -LiteralPath $exe -Destination (Join-Path $bundle 'rocket-lab.exe')
foreach ($relative in @('assets','examples','docs','src','tests')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot $relative) -Destination $bundle -Recurse
}
foreach ($relative in @('README.md','Cargo.toml','Cargo.lock','Launch.ps1')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot $relative) -Destination $bundle
}
$scriptDest = New-Item -ItemType Directory -Path (Join-Path $bundle 'scripts')
foreach ($file in @('build_model.py','cad_exchange.py','scene_exchange.py','build_part_previews.py','Build-Models.ps1','check_cfd.py','check_thermal.py','check_thermal_vehicle.py','plot_engineering.py','openfoam_analysis.py','check_external.py','check_vehicle_stress.py','check_vehicle_field.py','check_parts_model.py','check_model_flight.py','Dockerfile.analysis','package.ps1')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $scriptDest.FullName
}
$sourceDest = New-Item -ItemType Directory -Path (Join-Path $bundle 'sources')
foreach ($file in @('rasp.eng','Plumbbob_MKIII_R75 (1).ork','Plumbbob_MKIII_R75.CDX1','Plumbbob_MKIII_R75 PDR Flight Data.CSV')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "sources/$file") -Destination $sourceDest.FullName
}
$evidenceDest = New-Item -ItemType Directory -Path (Join-Path $bundle 'evidence')
foreach ($relative in @('analysis-images-acceptance.json','analysis-2d-flow-check.json','analysis-2d-thermal-check.json','analysis-2d-stress-check.json','analysis-images-native-errors.log','openrocket-acceptance.json','openrocket-native-errors.log','openrocket-parts-flight/summary.json','openrocket-parts-check.json','openrocket-parts-mission.json','openrocket-blender-check.json','openrocket-body-parts.png','openrocket-inner-parts.png','openrocket-recovery-parts.png','openrocket-pods.png','openrocket-freeform.png','final-acceptance.json','attachment-check.json','attachment-editor-mission.json','attachment-tube.png','attachment-nose-offset.png','external-ui-check.json','external-validation/external-reference-check.json','openfoam-flow.png','openfoam-thermal.png','calculix-stress.png','external-validation.log','external-images.txt','calculix-version.txt','analysis-image-build.log','editor-check.json','transition-blender-check.json','transition-editor-mission.json','editor-transition.png','editor-motor.png','editor-stage-recovery.png','reset-rocket-check.json','theme-dark-build.png','theme-light-build.png','theme-light-flight.png','core-tests.log','viewer-tests.log','clippy.log','timestamp-summary.json','timestamp-check.json','timestamp-cfd.png','timestamp-cfd-refined.png','timestamp-stress.png','moving-canopy.png','thermal-inner.png','model-catalogue.png','cfd/regime-pressure.png','validation/reference-comparison.png','validation/reference-check.json','validation/convergence.json','advanced-validation/convergence.json','thermal/full-reference-check.json','cfd-gpu-check/cpu-reference-check.json','advanced-dispersion/dispersion-summary.json')) {
    $from = Join-Path $projectRoot "output/$relative"
    if (Test-Path -LiteralPath $from) {
        $safeName = $relative.Replace('/','-')
        Copy-Item -LiteralPath $from -Destination (Join-Path $evidenceDest.FullName $safeName)
    }
}
@'
@echo off
cd /d "%~dp0"
rocket-lab.exe view --mission examples/kate-anvil-desert.json --start-time 0
if errorlevel 1 pause
'@ | Set-Content -LiteralPath (Join-Path $bundle 'Launch.cmd') -Encoding ascii
foreach ($relative in @('external-validation','analysis-images','model-flight','vehicle-stress-verification','external-materials-validation','full-vehicle-external-case')) {
    $from = Join-Path $projectRoot "output/$relative"
    if (Test-Path -LiteralPath $from) {Copy-Item -LiteralPath $from -Destination $evidenceDest.FullName -Recurse}
}
foreach ($relative in @('model-flight-check.json','model-native-check.json','model-parts-geometry-check.json','model-acceptance.json','model-all-tests.log','model-viewer-tests.log','model-clippy.log','model-based-flight.png','progress-flight-check.json','progress-model-check.json','progress-native-check.json','progress-core-tests.log','progress-viewer-tests.log','progress-clippy.log','progress-build.log','progress-panel-tests.log','progress-panels.png','progress-gpu-flow.png','progress-gpu-thermal.png','thermal-viewer-tests.log','thermal-clippy.log','thermal-build.log','thermal-vehicle-check.json','thermal-vehicle.png','thermal-vehicle-3d.png','theme-refined-dark.png','theme-refined-light.png','theme-refined-flight.png','thermal-native-errors.log')) {
    $from = Join-Path $projectRoot "output/$relative"
    if (Test-Path -LiteralPath $from) {Copy-Item -LiteralPath $from -Destination $evidenceDest.FullName}
}
foreach ($relative in @('exchange-builder-tests-complete.log','exchange-builder-clippy-last.log','exchange-builder-thermal-hardware.log','exchange-builder-final-build.log','exchange-e2e/roundtrip-results.json','exchange-e2e/gpu-flow/compute.json','exchange-e2e/internal-model-final/build-receipt.json','release-v0.1.2-tests.log','release-v0.1.2-clippy.log','release-v0.1.2-build.log','release-v0.1.2-check.json')) {
    $from = Join-Path $projectRoot "output/$relative"
    if (Test-Path -LiteralPath $from) { Copy-Item -LiteralPath $from -Destination (Join-Path $evidenceDest.FullName $relative.Replace('/','-')) }
}
$snapshotRoot = Join-Path $projectRoot 'output/snapshots'
foreach ($relative in @('shared-fields-native-check.json','shared-fields-gpu-test.log','shared-fields-cfd-2d.jpg','shared-fields-cfd-3d.jpg','shared-fields-thermal-2d.jpg','shared-fields-thermal-3d.jpg','shared-fields-stress-2d.jpg','shared-fields-stress-3d.jpg','staging-check.json','transition-check.json','transition-all-tests.log','transition-clippy.log','transition-cfd-2d.jpg','transition-model-3d.jpg','transition-model-build.log','release-v0.1.1-tests.log','release-v0.1.1-clippy.log','release-v0.1.1-build.log')) {
    $from = Join-Path $projectRoot "output/$relative"
    if (Test-Path -LiteralPath $from) {Copy-Item -LiteralPath $from -Destination $evidenceDest.FullName}
}
foreach ($relative in @('sidebar-tests.log','sidebar-clippy.log','sidebar-build.log','sidebar-native-errors.log','sidebar-dark.jpg','sidebar-light.jpg','sidebar-parts.jpg','sidebar-native-check.json','detail-all-tests.log','detail-viewer-final-tests.log','detail-gpu-thermal-test.log','detail-clippy.log','detail-build.log','detail-native-errors.log','detail-ultra-flow.png','detail-ultra-flow-motion.png','detail-ultra-thermal.png','detail-smooth-nose.png','detail-native-check.json')) {
    $from = Join-Path $projectRoot "output/$relative"
    if (Test-Path -LiteralPath $from) {Copy-Item -LiteralPath $from -Destination $evidenceDest.FullName}
}
foreach ($relative in @('units-stress-core-tests.log','units-stress-viewer-tests.log','units-controls-tests.log','units-stress-clippy.log','units-stress-build.log','units-stress-native-errors.log','full-vehicle-external.log','full-vehicle-field-check.log','vehicle-stress-verification.log','external-materials-validation.log','structural-verification-mission.json','units-imperial.png','units-metric.png','units-build-imperial.png','units-build-metric.png','vehicle-stress.png','vehicle-stress-3d.png','units-stress-native-check.json')) {
    $from = Join-Path $projectRoot "output/$relative"
    if (Test-Path -LiteralPath $from) {Copy-Item -LiteralPath $from -Destination $evidenceDest.FullName}
}
if (Test-Path -LiteralPath $snapshotRoot) {
    $gpuCases = New-Item -ItemType Directory -Path (Join-Path $evidenceDest.FullName 'gpu-snapshots')
    foreach ($mode in @('cfd','thermal','stress')) {
        $gpuLatest = Get-ChildItem -LiteralPath $snapshotRoot -Directory | Where-Object {
            $_.Name -match "-$mode-.*-(Preview|Standard|Fine|Ultra)$" -and (Test-Path -LiteralPath (Join-Path $_.FullName 'snapshot.json'))
        } | Sort-Object { (Get-Item -LiteralPath (Join-Path $_.FullName 'snapshot.json')).LastWriteTime } -Descending | Where-Object {
            (Get-Content -LiteralPath (Join-Path $_.FullName 'snapshot.json') -Raw | ConvertFrom-Json).gpu_used
        } | Select-Object -First 1
        if ($gpuLatest) {Copy-Item -LiteralPath $gpuLatest.FullName -Destination $gpuCases.FullName -Recurse}
    }
    $cases = New-Item -ItemType Directory -Path (Join-Path $evidenceDest.FullName 'external-snapshots')
    foreach ($mode in @('cfd','thermal','stress')) {
        $latest = Get-ChildItem -LiteralPath $snapshotRoot -Directory | Where-Object {$_.Name -match "-$mode-.*-OpenFoam$" -and (Test-Path -LiteralPath (Join-Path $_.FullName 'snapshot.json'))} | Sort-Object { (Get-Item -LiteralPath (Join-Path $_.FullName 'snapshot.json')).LastWriteTime } -Descending | Select-Object -First 1
        if ($latest) {Copy-Item -LiteralPath $latest.FullName -Destination $cases.FullName -Recurse}
    }
}
@'
# Runnable Windows bundle

Analysis detail is expanded: Ultra flow computes 2048 × 1024 cells over 32,000 iterations. Fine uses 1024 × 512, Standard 384 × 256. Flow motion defaults on in the 2D CFD view, with smooth texture sampling and animated RK2 trails through the captured velocity field. The displayed rocket uses a continuous nose/body/transition silhouette at every zoom; PNG edges use subpixel coverage. Texture-only wall-cell gaps sample nearby fluid values, while hover/CSV retain the actual grid mask. The numerical wall remains Cartesian. Display playback slows the flow; it is not a transient liquid or turbulence simulation. Ultra thermal uses 256 body/nose and 64 fin columns per stage, with a maximum diffusion step of 0.001 s; the GPU supports 1024 material columns. Ultra fin stress uses 1024 integration divisions. Structural and external wall meshes also refine. Larger jobs retain elapsed time and ETA reporting. Raw numerical outputs remain full resolution.

The 2D and 3D views share computed CFD scalars, thermal/stress mappings, color scales and layer choices. Thermal and stress vehicle views use smooth colored triangles; the 3D CFD view also shows the computed planar slice. Separation preserves each section's displayed position at its physical joint. The default Kate and Anvil model includes round conical transitions, matching CFD profiles and material-dependent thermal sections. See docs/builder-and-flight.md for transition dimensions and material assumptions.

Extract the entire folder and double-click Launch.cmd. File provides New rocket, Open rocket, Save and Save as for editable JSON designs, with an unsaved-changes prompt. Import/Export handles model files separately; see docs/model-exchange.md. The simulation viewport shows the current air-relative Mach regime, stage and timestamp. The default is the supplied three-stage desert study with a 5 lb Kate and Anvil payload. Open Simulate for flight timestamp analysis. Run rocket-lab.exe view --mission examples/advanced-laboratory.json for the example moving-canopy, terrain/profile and material extensions.

Parts is organized as Body & fins, Inner components, Mass & recovery, and Assemblies. Includes elliptical/freeform/tube fins, launch lug, engine block, streamer and repeated pod housings. The separate Booster stage card is removed. See docs/builder-and-flight.md for property controls and remaining OpenRocket parity limits. Drag cards from Parts into 3D; click an actual component to edit its properties and see its selection box. Transitions attach as tapered parts within the selected stage. Inner tubes attach inside bodies; centering rings fit around their parent mount. Motor selection is available on inner-tube mounts, filtered by bore and length. Enable Internals to select motors, payload and packed parachutes. Body tubes attach above/below the pointed section; noses attach at the top. Click a section to edit length, diameter and axial offset. Move in 3D repositions an extra component; the inspector can remove the selected stage. Short left clicks select, while left/right drags move the camera.

In Simulate → Analysis, choose 2D images for full-size pressure, speed/Mach, wall temperature/heating-history and whole-vehicle stress/fin envelope images. Wheel zooms, drag pans and hover reports values. Save PNG + metadata writes output/analysis-images. 3D vehicle restores the rocket view. See docs/timestamp-analysis.md for field scope and export. Step backward/forward, scrub the launch or enter a timestamp. Choose CFD, Thermal or Stress in Simulate, select Preview/Standard/Fine/Ultra, then Compute. Higher detail refines the actual solver grid, integration and iteration budget. Thermal starts at launch and ends at your timestamp. Results export to output/snapshots. External solvers uses OpenFOAM v2406 for compressible planar flow and material wall diffusion, and CalculiX for material-specific quadratic S8 vehicle shells and separate C3D20R primary fins. Docker Desktop and Python are required: see docs/external-solvers.md and scripts/Dockerfile.analysis. These solver jobs use the CPU. Built-in analysis defaults to hardware GPU flow and thermal diffusion; GPU errors are reported without silently switching to CPU. Beam stress integration and the 6DoF flight solver use CPU f64. Jobs show elapsed time and an approximate ETA. Enter stiffness for each shell/fin material in Build before external stress; optional strength and temperature factors belong to that material; no preview substitutes for a failed external job.

Drag either sidebar's inner edge or the bottom panel's top edge to resize; charts and the parts grid adapt. Sidebars reserve a scrollbar gutter, clip content before the handle and wrap long property labels while retaining visible units. Launch-condition units use large 20-point labels for launch, wind, weather and clouds in both Imperial and Metric. Parts uses one scroll area. Choose Dark or Light in the header; the theme is remembered. Imperial/Metric converts all display units and is also remembered; data files retain labeled SI values. Values support drag, type and +/− steps, with explanations on hover. Reset rocket restores the loaded design and centers the camera; Undo restores draft edits. Rewind flight or Home pauses at launch without changing the design.

The header starts with Build and Simulate, with matching graphite/sage Dark and Light themes. The bottom panel keeps its chosen height during chart updates. Thermal defaults to Vehicle shape: the actual exported mesh silhouette colored by computed material temperatures, including nose and fins. Choose the retained vehicle or one stage; uncomputed parts are gray. Wall section and Heating history remain available. The 3D vehicle view also uses computed temperatures with an automatic range. Geometry views project the existing material model and do not add circumferential thermal resolution.

Rust is not needed to run rocket-lab.exe. Windows with a working graphics driver is required. Blender is required to evaluate changed designs and convert supported scene formats; bundled meshes and cards work offline. FreeCADCmd is required for STEP/IGES/BREP/FCStd exchange. Native STL/OBJ import needs neither converter. Google imagery needs your own GOOGLE_MAPS_API_KEY. No credentials are included. Headless CLI commands and full source are included; Cargo commands need Rust/MSVC installed.

The GPU solver is planar perfect-gas inviscid Euler, not full 3D viscous hypersonic CFD. External flow is a laminar planar perfect-gas section; thermal uses independent wall strips and prescribed heating; FEA uses an aft-clamped bonded shell with prescribed inertia/aero loads and separate clamped primary fins. These are post-processing without full 3D conjugate heat transfer, vehicle joints, fracture, flutter or ablation feedback. Examples include synthetic coefficients/properties. The supplied RASAero reference check fails the selected 5% peak tolerances; configuration identity is unresolved. See docs/advanced-laboratory.md for capabilities, reproduction commands and exact remaining work. The evidence folder contains implementation checks, not measured-flight validation.
'@ | Set-Content -LiteralPath (Join-Path $bundle 'PACKAGE-README.md') -Encoding utf8
$manifest = [ordered]@{
    created_utc = (Get-Date).ToUniversalTime().ToString('o')
    executable_sha256 = (Get-FileHash -LiteralPath (Join-Path $bundle 'rocket-lab.exe') -Algorithm SHA256).Hash
    source_commit = (& git -C $projectRoot rev-parse HEAD)
    executable_version = (& $exe --version)
    build = 'Windows x64 Rust 1.91.1; viewer-enabled dev profile optimized; Bevy 0.18.1'
    contents = @(Get-ChildItem -LiteralPath $bundle -Recurse -File | ForEach-Object { $_.FullName.Substring($bundle.Length + 1) })
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundle 'manifest.json') -Encoding utf8
$archive = $bundle + '.zip'
Compress-Archive -LiteralPath $bundle -DestinationPath $archive -CompressionLevel Optimal
Write-Output $bundle
Write-Output $archive
