# Installs sultrakey on Windows (developer laptops), without Administrator rights:
#
#   powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/sultrakey/releases/latest/download/install.ps1 | iex"
#
# A given version: set $env:SULTRAKEY_VERSION = '0.1.0' before running the command above.
# This script only downloads the binary, checks its SHA256, then runs `sultrakey.exe install`.
# (ASCII characters only: Windows PowerShell 5.1 reads files without a BOM as ANSI.)

function Install-Sultrakey {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $base = 'https://github.com/hmrnsp/sultrakey/releases'
    if ($env:SULTRAKEY_RELEASE_BASE) { $base = $env:SULTRAKEY_RELEASE_BASE }
    if ($env:SULTRAKEY_VERSION) {
        $url = "$base/download/v$($env:SULTRAKEY_VERSION.TrimStart('v'))"
    } else {
        $url = "$base/latest/download"
    }
    # Windows on ARM runs the x86_64 build.
    $name = 'sultrakey-x86_64-pc-windows-msvc.exe'

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('sultrakey-' + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $exe = Join-Path $tmp 'sultrakey.exe'
        $sums = Join-Path $tmp 'SHA256SUMS'
        Write-Host "Downloading $name ..."
        Invoke-WebRequest -UseBasicParsing -Uri "$url/$name" -OutFile $exe
        Invoke-WebRequest -UseBasicParsing -Uri "$url/SHA256SUMS" -OutFile $sums

        $expected = $null
        foreach ($line in Get-Content $sums) {
            $parts = $line.Trim() -split '\s+'
            if ($parts.Count -ge 2 -and $parts[1].TrimStart('*') -eq $name) {
                $expected = $parts[0].ToLower()
            }
        }
        if (-not $expected) { throw "SHA256SUMS does not list $name." }
        $actual = (Get-FileHash -Algorithm SHA256 -Path $exe).Hash.ToLower()
        if ($actual -ne $expected) { throw "Checksum of $name does not match; nothing was installed." }

        & $exe install
        if ($LASTEXITCODE -ne 0) { throw "sultrakey install failed (exit $LASTEXITCODE)." }
    } catch {
        Write-Host "FAILED: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host 'Fix: check the connection to github.com, then run the install command again.'
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

Install-Sultrakey
