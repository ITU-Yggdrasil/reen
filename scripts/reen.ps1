# Dot-source in PowerShell: . .\scripts\reen.ps1

function reen {
    param(
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Arguments
    )

    $workdir = (Get-Location).Path
    $envFileArgs = @()
    if (Test-Path ".env") {
      $envFileArgs = @("--env-file", "$workdir/.env")
    }
    $envVarArgs = @()
    if ($env:MISTRAL_API_KEY) {
      $envVarArgs = @("-e", "MISTRAL_API_KEY=$env:MISTRAL_API_KEY")
    }

    $imageId = docker image ls -q reen:latest
    if (-not $imageId) {
      Write-Error "Docker image 'reen:latest' was not found locally."
      Write-Host "Build it with: docker build -t reen:latest ."
      return
    }

    docker run --rm -it `
      @envFileArgs `
      @envVarArgs `
      -v "${workdir}:/work" `
      -w /work `
      reen:latest @Arguments
}
