param([switch]$GraphicsOnly, [string]$From = '')
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $workspace
New-Item -ItemType Directory -Force -Path (Join-Path $workspace 'artifacts') | Out-Null
# Winit allows one event loop per process. Each test needs its own executable run.
$checks = @()
if (!$GraphicsOnly) {
    $checks += @(
        @('oxy_editor','native_ux_m1_workflow'),
        @('oxy_editor','native_input_guide_workflow'),
        @('oxy_editor','native_mesh_foundation_workflow'),
        @('oxy_editor','native_basic_modeling_workflow'),
        @('oxy_editor','native_cut_bevel_workflow'),
        @('oxy_editor','native_mesh_painting_workflow'),
        @('oxy_editor','native_guide_recipes_workflow'),
        @('oxy_editor','native_layout_matrix_workflow'),
        @('oxy_editor','native_final_creation_workflow'),
        @('oxy_editor','native_spatial_workflow'),
        @('oxy_editor','native_editor_workflow'),
        @('oxy_editor','native_portable_home_workflow'),
        @('oxy_player','native_player_portable_workflow'),
        @('oxy_player','native_player_final_authored_mesh')
    )
}
$checks += @(
    @('oxy_render','native_gpu_depth_texture_and_resize'),
    @('oxy_render','native_gpu_editable_mesh_equivalence')
)
$started = !$From
$completed = 0
foreach ($check in $checks) {
    $crate = $check[0]
    $test = $check[1]
    if ($test -eq $From) { $started = $true }
    if (!$started) { continue }
    $arguments = @('test','--release','--locked','-p',$crate,$test,'--','--ignored','--nocapture')
    & (Join-Path $PSScriptRoot 'cargo.ps1') @arguments 2>&1 | Tee-Object -FilePath (Join-Path $workspace "artifacts/v020-final-$test.log")
    if ($LASTEXITCODE -ne 0) { throw "Falhou: $crate / $test" }
    $completed++
}
if (!$started) { throw "Teste inicial desconhecido: $From" }
Write-Host "QA nativo concluído: $completed testes em processos separados."
