$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

# 实验固定与 Linux 发布相同的 Rust、Zig 和 cargo-zigbuild。
$tools = Join-Path $env:RUNNER_TEMP 'windows-cross-tools'
New-Item -ItemType Directory -Force $tools | Out-Null

function Get-VerifiedArchive($Url, $Digest, $Name) {
    $archive = Join-Path $tools $Name
    curl.exe --fail --location --silent --show-error --retry 3 $Url --output $archive
    if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Digest) {
        throw "Checksum mismatch: $Name"
    }
    return $archive
}

function Add-ToolPath($Path) {
    $env:PATH = "$Path;$env:PATH"
    $Path | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
}

function Get-PeMachine($Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    $offset = [BitConverter]::ToInt32($bytes, 0x3c)
    return ('0x{0:x4}' -f [BitConverter]::ToUInt16($bytes, $offset + 4))
}

rustup toolchain install 1.95.0-aarch64-pc-windows-msvc --profile minimal
rustup target add --toolchain 1.95.0-aarch64-pc-windows-msvc aarch64-apple-darwin x86_64-unknown-linux-gnu
'RUSTUP_TOOLCHAIN=1.95.0-aarch64-pc-windows-msvc' | Out-File $env:GITHUB_ENV -Encoding utf8 -Append

$archive = Get-VerifiedArchive 'https://github.com/rust-cross/cargo-zigbuild/releases/download/v0.24.0/cargo-zigbuild-aarch64-pc-windows-msvc.zip' 'c031663c9324463c5f5b45a8c94197816c7a98807a8bdbdf127542b8fe81b9fe' 'cargo-zigbuild.zip'
Expand-Archive $archive -DestinationPath (Join-Path $tools 'cargo-zigbuild')
$cargoZigbuild = Get-ChildItem (Join-Path $tools 'cargo-zigbuild') -Recurse -Filter cargo-zigbuild.exe | Select-Object -First 1
Add-ToolPath $cargoZigbuild.DirectoryName

$archive = Get-VerifiedArchive 'https://ziglang.org/download/0.16.0/zig-aarch64-windows-0.16.0.zip' 'aee38316ee4111717900f45dd3130145c39289e105541d737eb8c5ed653c78ef' 'zig.zip'
Expand-Archive $archive -DestinationPath (Join-Path $tools 'zig')
$zig = Get-ChildItem (Join-Path $tools 'zig') -Recurse -Filter zig.exe | Select-Object -First 1
Add-ToolPath $zig.DirectoryName
"CARGO_ZIGBUILD_ZIG_PATH=$($zig.FullName)" | Out-File $env:GITHUB_ENV -Encoding utf8 -Append

$archive = Get-VerifiedArchive 'https://github.com/joseluisq/macosx-sdks/releases/download/26.1/MacOSX26.1.sdk.tar.xz' 'beee7212d265a6d2867d0236cc069314b38d5fb3486a6515734e76fa210c784c' 'macos-sdk.tar.xz'
tar -xJf $archive -C $tools
"SDKROOT=$(Join-Path $tools 'MacOSX26.1.sdk')" | Out-File $env:GITHUB_ENV -Encoding utf8 -Append

$rustc = (& rustup which rustc --toolchain 1.95.0-aarch64-pc-windows-msvc).Trim()
$inventory = @{
    source_commit = (& git rev-parse HEAD).Trim()
    rust_source_tree = (& git rev-parse HEAD:codex-rs).Trim()
    image_version = $env:ImageVersion
    processor = @(Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors)
    memory_bytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    tools = @{
        rustc = @{ path = $rustc; machine = (Get-PeMachine $rustc) }
        zig = @{ path = $zig.FullName; machine = (Get-PeMachine $zig.FullName) }
        cargo_zigbuild = @{ path = $cargoZigbuild.FullName; machine = (Get-PeMachine $cargoZigbuild.FullName) }
    }
}
foreach ($tool in $inventory.tools.Values) {
    if ($tool.machine -ne '0xaa64') { throw 'Compiler must be native Windows ARM64' }
}
$inventory | ConvertTo-Json -Depth 6 | Tee-Object -FilePath runner-inventory.json
& $rustc -vV
zig version
Get-Command clang, cmake, ninja, pkg-config -ErrorAction SilentlyContinue | Select-Object Name, Source
