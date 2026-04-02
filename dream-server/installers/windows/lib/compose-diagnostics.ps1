# ============================================================================
# Dream Server Windows -- Docker Compose diagnostics
# ============================================================================
# Part of: installers/windows/lib/
# Purpose: When docker compose fails, print actionable context (config, docker
#          state) so installs on diverse hardware produce useful bug reports.
# Requires: ui.ps1 (Write-AI, Write-AIWarn, Write-Chapter) sourced first.
# ============================================================================

function Get-DreamComposeEnvFileArgs {
    param([string]$InstallDir)
    $envPath = Join-Path $InstallDir ".env"
    if (Test-Path $envPath) {
        return @("--env-file", ".env")
    }
    return @()
}

function Invoke-DreamDockerCompose {
    <#
    .SYNOPSIS
        Run docker compose with PS 5.1-safe stderr handling; return exit code.
    #>
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallDir,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [string[]]$ComposeFlags,
        [Parameter(Mandatory = $true)]
        [string[]]$ComposeArgs
    )
    Push-Location $InstallDir
    try {
        $prevEAP = $ErrorActionPreference
        $ErrorActionPreference = 'SilentlyContinue'
        & docker compose @ComposeFlags @ComposeArgs 2>&1 | ForEach-Object { Write-Host "  $_" }
        $exitCode = $LASTEXITCODE
        $ErrorActionPreference = $prevEAP
        return $exitCode
    }
    finally {
        Pop-Location
    }
}

function Write-DreamComposeDiagnostics {
    <#
    .SYNOPSIS
        Print bounded docker/compose diagnostics after a compose command failed.
    #>
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallDir,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [string[]]$ComposeFlags,
        [string]$Phase = "install"
    )

    Write-Chapter "COMPOSE FAILURE DIAGNOSTICS"
    Write-AI "Phase: $Phase — save this section if you report an issue."
    Write-AI "Docs: dream-server/docs/WINDOWS-TROUBLESHOOTING-GUIDE.md (section: Docker Compose failed)"
    Write-Host ""

    Push-Location $InstallDir
    try {
        $prevEAP = $ErrorActionPreference
        $ErrorActionPreference = 'SilentlyContinue'

        Write-Host "  --- docker version ---" -ForegroundColor DarkGray
        $dv = & docker version 2>&1 | ForEach-Object { $_.ToString() }
        if ($dv) { $dv | Select-Object -First 25 | ForEach-Object { Write-Host "  $_" } }
        else { Write-Host "  (docker version produced no output)" -ForegroundColor Yellow }
        Write-Host ""

        Write-Host "  --- docker info (first 35 lines) ---" -ForegroundColor DarkGray
        $di = & docker info 2>&1 | ForEach-Object { $_.ToString() }
        if ($di) { $di | Select-Object -First 35 | ForEach-Object { Write-Host "  $_" } }
        else { Write-Host "  (docker info failed — is Docker Desktop running?)" -ForegroundColor Yellow }
        Write-Host ""

        $envArgs = Get-DreamComposeEnvFileArgs -InstallDir $InstallDir
        Write-Host "  --- docker compose ... config (last 55 lines) ---" -ForegroundColor DarkGray
        $cfgOut = & docker compose @ComposeFlags @envArgs config 2>&1 | ForEach-Object { $_.ToString() }
        if ($cfgOut) {
            $cfgLines = @($cfgOut)
            $take = [Math]::Min(55, $cfgLines.Count)
            if ($cfgLines.Count -gt 0) {
                $start = [Math]::Max(0, $cfgLines.Count - $take)
                for ($i = $start; $i -lt $cfgLines.Count; $i++) {
                    Write-Host "  $($cfgLines[$i])"
                }
            }
        }
        else {
            Write-AIWarn "docker compose config produced no output (merge/parse error likely above)."
        }
        Write-Host ""

        Write-Host "  --- docker compose ... ps -a ---" -ForegroundColor DarkGray
        $psOut = & docker compose @ComposeFlags @envArgs ps -a 2>&1 | ForEach-Object { $_.ToString() }
        if ($psOut) {
            $psOut | Select-Object -First 40 | ForEach-Object { Write-Host "  $_" }
        }
        else {
            Write-Host "  (no ps output)" -ForegroundColor DarkGray
        }

        $ErrorActionPreference = $prevEAP
    }
    finally {
        Pop-Location
    }

    Write-Host ""
    Write-AI "Next: confirm Docker Desktop is running, WSL2 backend is on, and ports in .env are free."
}
