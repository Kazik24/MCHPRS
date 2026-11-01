param(
    [string]$Clang = $env:MCHPRS_LLVM_CLANG,
    [string]$Linker = $env:MCHPRS_LLVM_LINKER
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Clang) {
    $installed = Get-Command clang.exe -ErrorAction SilentlyContinue
    if ($installed) { $Clang = $installed.Source }
    else { $Clang = Join-Path $repo 'target/llvm-23.1.4/bin/clang.exe' }
}
if (-not (Test-Path -LiteralPath $Clang)) {
    throw 'LLVM x64 Clang was not found. Install LLVM or pass -Clang <clang.exe>.'
}
$Clang = (Resolve-Path -LiteralPath $Clang).Path
if (-not $Linker) { $Linker = Join-Path (Split-Path -Parent $Clang) 'lld-link.exe' }
if (-not (Test-Path -LiteralPath $Linker)) {
    throw 'The matching lld-link.exe was not found. Pass -Linker <lld-link.exe>.'
}
$Linker = (Resolve-Path -LiteralPath $Linker).Path
$include = Join-Path $repo 'crates/core/src/redpiler/sampled'
$target = Join-Path $repo 'target'
New-Item -ItemType Directory -Path $target -Force | Out-Null
$object = Join-Path $target 'native_engine.obj'
$dll = Join-Path $target 'native_engine.dll'
$compileArgs = @('--target=x86_64-pc-windows-msvc', '-std=c11', '-O3', '-ffreestanding', '-fno-builtin', '-Werror', '-I', $include,
    '-c', (Join-Path $include 'native_engine.c'), '-o', $object)
$linkArgs = @('/dll', '/noentry', '/nodefaultlib', '/machine:x64', "/out:$dll", $object)
$timer = [System.Diagnostics.Stopwatch]::StartNew()
& $Clang @compileArgs
if ($LASTEXITCODE -ne 0) { throw 'Clang failed; see diagnostics above.' }
& $Linker @linkArgs
if ($LASTEXITCODE -ne 0) { throw 'lld-link failed; stop any process using the output DLL and check diagnostics above.' }
$timer.Stop()
$sourceInfo = @('native_engine.c', 'native_movement.c', 'native_abi.h') | ForEach-Object {
    $path = Join-Path $include $_
    [ordered]@{ name = $_; bytes = (Get-Item -LiteralPath $path).Length; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
}
$manifest = [ordered]@{
    built_utc = [DateTime]::UtcNow.ToString('o')
    clang = $Clang; clang_version = (& $Clang --version | Select-Object -First 1)
    clang_sha256 = (Get-FileHash -LiteralPath $Clang -Algorithm SHA256).Hash
    linker = $Linker; linker_version = (& $Linker --version | Select-Object -First 1)
    linker_sha256 = (Get-FileHash -LiteralPath $Linker -Algorithm SHA256).Hash
    compile_args = $compileArgs; link_args = $linkArgs; sources = @($sourceInfo)
    build_ms = $timer.Elapsed.TotalMilliseconds
    object_bytes = (Get-Item -LiteralPath $object).Length
    dll = $dll; dll_bytes = (Get-Item -LiteralPath $dll).Length
    dll_sha256 = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $target 'native-engine-build.json') -Encoding UTF8
Write-Output ('Built {0} ({1} bytes) in {2:N1} ms' -f $dll, $manifest.dll_bytes, $manifest.build_ms)
Write-Output ('Outside this repository, set MCHPRS_LLVM_DLL to {0}' -f $dll)
