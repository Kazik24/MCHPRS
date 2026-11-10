param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Clang = $env:MCHPRS_LLVM_CLANG,
    [string]$Linker = $env:MCHPRS_LLVM_LINKER,
    [switch]$GenerateOnly
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$include = Join-Path $repo 'crates/core/src/redpilator/sampled/llvm'
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$source = [System.IO.File]::ReadAllText((Join-Path $include 'engine.c'))
$pattern = '(?s)uint32_t ne_tick\(void \*memory\) \{.*?\r?\n\}'
$matches = [regex]::Matches($source, $pattern)
if ($matches.Count -ne 1) { throw 'Expected exactly one scalar tick body.' }
$scalar = $matches[0].Value
$inline = $scalar.Replace('uint32_t ne_tick(void *memory)', 'static inline __attribute__((always_inline)) uint32_t batch_tick_one(void *memory)')
$source = $source.Replace($scalar, "$inline`nuint32_t ne_tick(void *memory) { return batch_tick_one(memory); }")
$wrappers = ''
foreach ($n in @((2..6) + @(8, 16, 32))) {
    $wrappers += "`nstatic uint32_t batch_tick_$n(void *memory) {`n uint32_t status;`n"
    foreach ($step in 1..$n) { $wrappers += " if ((status = batch_tick_one(memory)) != NE_OK) return status;`n" }
    $wrappers += " return NE_OK;`n}`n"
}
$wrappers += @'
MCHPRS_NATIVE_EXPORT uint32_t ne_benchmark_tick_batch(void *memory, uint32_t count, uint32_t mode) {
    if (!memory || mode < 2u || mode > 3u || (mode == 3u && count > 32u)) return NE_INVALID;
    if (mode == 3u) {
        switch (count) {
            case 0: return NE_OK;
            case 1: return batch_tick_one(memory);
            case 2: return batch_tick_2(memory);
            case 3: return batch_tick_3(memory);
            case 4: return batch_tick_4(memory);
            case 5: return batch_tick_5(memory);
            case 6: return batch_tick_6(memory);
            case 8: return batch_tick_8(memory);
            case 16: return batch_tick_16(memory);
            case 32: return batch_tick_32(memory);
        }
    }
    for (uint32_t i = 0; i < count; ++i) {
        uint32_t status = batch_tick_one(memory);
        if (status != NE_OK) return status;
    }
    return NE_OK;
}
'@
$generated = Join-Path $OutputDirectory 'engine-batches.c'
[System.IO.File]::WriteAllText($generated, $source + $wrappers)
if ($GenerateOnly) { Write-Output $generated; return }
if (-not $Clang) { $Clang = Join-Path $repo 'target/llvm-23.1.4/bin/clang.exe' }
$Clang = (Resolve-Path -LiteralPath $Clang).Path
if (-not $Linker) { $Linker = Join-Path (Split-Path -Parent $Clang) 'lld-link.exe' }
$Linker = (Resolve-Path -LiteralPath $Linker).Path
$object = Join-Path $OutputDirectory 'engine-batches.obj'
$dll = Join-Path $OutputDirectory 'engine-batches.dll'
$compileArgs = @('--target=x86_64-pc-windows-msvc', '-std=c11', '-O3', '-ffreestanding', '-fno-builtin', '-Werror', '-I', $include, '-c', $generated, '-o', $object)
& $Clang @compileArgs
if ($LASTEXITCODE -ne 0) { throw 'Batch experiment Clang failed.' }
$linkArgs = @('/dll', '/noentry', '/nodefaultlib', '/machine:x64', "/out:$dll", $object)
& $Linker @linkArgs
if ($LASTEXITCODE -ne 0) { throw 'Batch experiment linker failed.' }
$manifest = [ordered]@{
    built_utc = [DateTime]::UtcNow.ToString('o'); clang = $Clang; linker = $Linker
    compile_args = $compileArgs; link_args = $linkArgs
    sources = @(@('engine.c', 'llvm_movement.c', 'command_engine.c', 'abi.h') | ForEach-Object {
        $path = Join-Path $include $_
        @{ name = $_; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
    })
    generated_sha256 = (Get-FileHash -LiteralPath $generated -Algorithm SHA256).Hash
    dll = $dll; dll_sha256 = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'build.json') -Encoding UTF8
Write-Output $dll
