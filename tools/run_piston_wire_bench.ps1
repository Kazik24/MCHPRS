param(
    [int]$CpuSamples = 3,
    [int]$PistonSamples = 2,
    [string]$Directory = 'target/piston-wire-comparison'
)

# Build and copy the unmodified and candidate bench executables into Directory
# as cpus-baseline.exe / cpus-optimized.exe and piston-baseline.exe / piston-optimized.exe.
# CPU executables enforce the original frozen references. Never regenerate them.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$resultsDirectory = [IO.Path]::GetFullPath((Join-Path $workspace $Directory))
$utf8 = [Text.UTF8Encoding]::new($false)
$cpuRuns = [Collections.Generic.List[object]]::new()
$pistonRuns = [Collections.Generic.List[object]]::new()

function Write-Results {
    $report = [ordered]@{
        schema = 1
        baseline_commit = (Get-Content (Join-Path $resultsDirectory 'base-commit.txt') -Raw).Trim()
        profile = 'bench/release, opt-level 3, fat LTO'
        logical_processor = 2
        process_priority = 'AboveNormal'
        notes = 'Sequential fresh processes, alternating variant order. Simulation only; networking and visual flush excluded. Both variants use deterministic offline chat capture.'
        cpu_runs = @($cpuRuns.ToArray())
        piston_runs = @($pistonRuns.ToArray())
    }
    [IO.File]::WriteAllText((Join-Path $resultsDirectory 'results.json'), ($report | ConvertTo-Json -Depth 30), $utf8)
}

function Run-Bench([string]$Executable, [string[]]$BenchArguments, [string]$LogName) {
    $outputPath = Join-Path $resultsDirectory "$LogName.stdout.txt"
    $errorPath = Join-Path $resultsDirectory "$LogName.stderr.txt"
    $process = Start-Process -FilePath (Join-Path $resultsDirectory $Executable) `
        -ArgumentList $BenchArguments -WorkingDirectory $workspace -WindowStyle Hidden `
        -RedirectStandardOutput $outputPath -RedirectStandardError $errorPath -PassThru
    try {
        $process.ProcessorAffinity = [IntPtr]4
        $process.PriorityClass = 'AboveNormal'
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) {
            throw "$Executable exited $($process.ExitCode). See $outputPath and $errorPath"
        }
    } finally {
        $process.Dispose()
    }
}

for ($sample = 1; $sample -le $CpuSamples; $sample++) {
    $variants = if ($sample % 2) { @('baseline', 'optimized') } else { @('optimized', 'baseline') }
    foreach ($cpu in @('anpu_pong', 'pm1_sort')) {
        foreach ($variant in $variants) {
            $name = "cpu-$cpu-$variant-$sample"
            $reportPath = Join-Path $resultsDirectory "$name.json"
            Run-Bench "cpus-$variant.exe" @('--cpu', $cpu, '--iterations', '1', '--label', $name, '--output', $reportPath) $name
            $report = Get-Content $reportPath -Raw | ConvertFrom-Json
            $cpuRuns.Add([ordered]@{ variant = $variant; sample = $sample; report = $report })
            Write-Results
            $timing = $report.samples[0]
            Write-Output "$name passed: $($timing.median_seconds)s; active $($timing.active_median_tps) TPS"
        }
    }
}

for ($sample = 1; $sample -le $PistonSamples; $sample++) {
    $variants = if ($sample % 2) { @('baseline', 'optimized') } else { @('optimized', 'baseline') }
    foreach ($variant in $variants) {
        $name = "piston-allocation-$variant-$sample"
        Run-Bench "piston-$variant.exe" @('--bench', '--noplot', '--save-baseline', $name) $name
        $estimates = [Collections.Generic.List[object]]::new()
        foreach ($kind in @('independent', 'chain')) {
            $sizes = if ($kind -eq 'independent') { @(1, 64, 256, 1024) } else { @(8, 32, 128) }
            foreach ($size in $sizes) {
                $path = Join-Path $workspace "target/criterion/piston-cycle/$kind/$size/$name/estimates.json"
                $estimate = Get-Content $path -Raw | ConvertFrom-Json
                $estimates.Add([ordered]@{ kind = $kind; size = $size; estimates = $estimate })
            }
        }
        $pistonRuns.Add([ordered]@{ variant = $variant; sample = $sample; measurements = @($estimates.ToArray()) })
        Write-Results
        Write-Output "$name completed all seven workloads"
    }
}
