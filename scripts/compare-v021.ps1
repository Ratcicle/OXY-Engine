param(
    [string]$Before = 'benchmarks/results/v021-native-m0-base.json',
    [string]$After = 'benchmarks/results/v021-native-final.json',
    [string]$Output = 'benchmarks/results/v021-comparison.csv'
)
$ErrorActionPreference = 'Stop'
$baseline = Get-Content -LiteralPath $Before -Raw | ConvertFrom-Json
$final = Get-Content -LiteralPath $After -Raw | ConvertFrom-Json
foreach ($report in @($baseline, $final)) {
    if (!$report.completed_all_fixtures -or $null -ne $report.error) {
        throw 'A comparação exige que os dois relatórios tenham concluído todas as cargas sem erro.'
    }
}
foreach ($field in @('warmup_cycles', 'sample_cycles', 'phase_limit_seconds', 'interface_scale', 'compiler')) {
    if ($baseline.$field -ne $final.$field) { throw "Método incompatível: $field" }
}
if (($baseline.window_logical_points -join ',') -ne ($final.window_logical_points -join ',')) {
    throw 'A janela da referência e a janela final têm tamanhos diferentes.'
}
$rows = foreach ($oldFixture in $baseline.rows) {
    $newFixture = @($final.rows | Where-Object { $_.fixture -eq $oldFixture.fixture })
    if ($newFixture.Count -ne 1 -or $newFixture[0].triangles -ne $oldFixture.triangles) {
        throw "Carga ausente ou alterada: $($oldFixture.fixture)"
    }
    foreach ($field in @('entities', 'vertices', 'edges', 'faces')) {
        if ($newFixture[0].$field -ne $oldFixture.$field) { throw "Geometria diferente: $($oldFixture.fixture) / $field" }
    }
    foreach ($oldPhase in $oldFixture.phases) {
        $newPhase = @($newFixture[0].phases | Where-Object { $_.phase -eq $oldPhase.phase })
        if ($newPhase.Count -ne 1) { throw "Fase ausente: $($oldPhase.phase)" }
        if ($newPhase[0].pixels_per_point -ne $oldPhase.pixels_per_point -or
            ($newPhase[0].content_logical_points -join ',') -ne ($oldPhase.content_logical_points -join ',')) {
            throw "Tamanho/escala efetiva diferente: $($oldFixture.fixture) / $($oldPhase.phase)"
        }
        foreach ($measure in $oldPhase.measurements.PSObject.Properties) {
            $old = $measure.Value
            $new = $newPhase[0].measurements.($measure.Name)
            if ($null -eq $new) {
                if ($newPhase[0].limited -and $newPhase[0].measured_cycles -eq 0) {
                    # The duration cap may expire during warmup: retain the failure
                    # to collect a sample instead of inventing a zero duration.
                    $new = [pscustomobject]@{ samples = 0; median_ns = $null; p95_ns = $null; p99_ns = $null }
                } else { throw "Medição ausente: $($measure.Name)" }
            }
            [pscustomobject]@{
                fixture = $oldFixture.fixture
                triangles = $oldFixture.triangles
                phase = $oldPhase.phase
                measure = $measure.Name
                before_samples = $old.samples
                after_samples = $new.samples
                before_median_ms = if ($null -ne $old.median_ns) { [Math]::Round($old.median_ns / 1e6, 6) } else { $null }
                after_median_ms = if ($null -ne $new.median_ns) { [Math]::Round($new.median_ns / 1e6, 6) } else { $null }
                ratio_before_over_after = if ($new.median_ns -gt 0 -and $old.median_ns -gt 0) { [Math]::Round($old.median_ns / $new.median_ns, 4) } else { $null }
                before_p95_ms = if ($null -ne $old.p95_ns) { [Math]::Round($old.p95_ns / 1e6, 6) } else { $null }
                after_p95_ms = if ($null -ne $new.p95_ns) { [Math]::Round($new.p95_ns / 1e6, 6) } else { $null }
                before_p99_ms = if ($null -ne $old.p99_ns) { [Math]::Round($old.p99_ns / 1e6, 6) } else { $null }
                after_p99_ms = if ($null -ne $new.p99_ns) { [Math]::Round($new.p99_ns / 1e6, 6) } else { $null }
                before_limit = $oldPhase.limited
                after_limit = $newPhase[0].limited
                before_selected_components = $oldPhase.last_selected_components
                after_selected_components = $newPhase[0].last_selected_components
                before_viewport_points = $oldPhase.viewport_logical_points -join 'x'
                after_viewport_points = $newPhase[0].viewport_logical_points -join 'x'
                before_history_estimated_bytes = $oldPhase.history_retained_estimated_bytes
                after_history_estimated_bytes = $newPhase[0].history_retained_estimated_bytes
                projection_builds = $newPhase[0].component_counters.projection_builds
                projection_hits = $newPhase[0].component_counters.projection_hits
                occlusion_builds = $newPhase[0].component_counters.occlusion_builds
                occlusion_hits = $newPhase[0].component_counters.occlusion_hits
                hover_queries = $newPhase[0].component_counters.hover_queries
                rectangle_queries = $newPhase[0].component_counters.rectangle_queries
                through_queries = $newPhase[0].component_counters.through_queries
                component_gpu_stream_uploads_cumulative = $newPhase[0].component_counters.geometry_stream_uploads_cumulative
                component_gpu_flags_uploads_cumulative = $newPhase[0].component_counters.selection_flag_uploads_cumulative
                component_draws_last_frame = $newPhase[0].component_counters.draw_submissions_last_frame
                component_gpu_allocated_buffer_bytes = $newPhase[0].component_counters.gpu_buffer_allocated_bytes
                component_cpu_cache_estimated_bytes = $newPhase[0].component_counters.cpu_cache_estimated_bytes
            }
        }
    }
}
$rows | ForEach-Object {
    foreach ($property in $_.PSObject.Properties) {
        if ($property.Value -is [double] -or $property.Value -is [single] -or $property.Value -is [decimal]) {
            $property.Value = $property.Value.ToString('0.######', [Globalization.CultureInfo]::InvariantCulture)
        }
    }
    $_
} | Export-Csv -LiteralPath $Output -NoTypeInformation -Encoding utf8
Write-Output $Output
