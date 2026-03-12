# Dot-source in PowerShell: . .\scripts\reem.ps1

function reem {
    param(
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Arguments
    )

    $workdir = (Get-Location).Path

    docker image inspect reen:latest *> $null
    if ($LASTEXITCODE -ne 0) {
      Write-Error "Docker image 'reen:latest' was not found locally."
      Write-Host "Build it with: docker build -t reen:latest ."
      return
    }

    docker run --rm -it `
      -e "MISTRAL_API_KEY=$env:MISTRAL_API_KEY" `
      -v "${workdir}:/work" `
      -w /work `
      reen:latest @Arguments
}
