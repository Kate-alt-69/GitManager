param(
    [Parameter(Mandatory = $true)]
    [string] $FilePath
)

$ErrorActionPreference = "Stop"
$thumbprint = ($env:WINDOWS_SIGNING_THUMBPRINT -replace '\\s', '').ToUpperInvariant()

if ([string]::IsNullOrWhiteSpace($thumbprint)) {
    Write-Host "[GitManager] No WINDOWS_SIGNING_THUMBPRINT configured; leaving this build unsigned."
    exit 0
}
if ($thumbprint -notmatch '^[A-F0-9]{40}$') {
    throw "WINDOWS_SIGNING_THUMBPRINT must be a 40-character SHA-1 certificate thumbprint."
}

$resolved = (Resolve-Path -LiteralPath $FilePath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
    throw "Signing target does not exist: $resolved"
}
$certificate = Get-ChildItem -Path Cert:\CurrentUser\My |
    Where-Object { $_.Thumbprint -eq $thumbprint -and $_.HasPrivateKey } |
    Select-Object -First 1
if (-not $certificate) {
    throw "Signing certificate $thumbprint is not installed in Cert:\CurrentUser\My with its private key."
}
$now = Get-Date
if ($certificate.NotBefore -gt $now -or $certificate.NotAfter -le $now) {
    throw "The selected Windows signing certificate is not currently valid."
}
$hasCodeSigning = @($certificate.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId.Value }) -contains "1.3.6.1.5.5.7.3.3"
if (-not $hasCodeSigning) {
    throw "The selected certificate does not include the Code Signing extended key usage."
}

$kitsRoot = Join-Path "${env:ProgramFiles(x86)}" "Windows Kits\10\bin"
if (-not (Test-Path -LiteralPath $kitsRoot)) {
    $kitsRoot = Join-Path $env:ProgramFiles "Windows Kits\10\bin"
}
$signtool = Get-ChildItem -LiteralPath $kitsRoot -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName "x64\signtool.exe" } |
    Where-Object { Test-Path -LiteralPath $_ } |
    Select-Object -First 1
if (-not $signtool) {
    throw "Windows SDK SignTool was not found. Install the Windows 10/11 SDK signing tools."
}

$existing = Get-AuthenticodeSignature -LiteralPath $resolved
if ($existing.SignerCertificate -and $existing.SignerCertificate.Thumbprint -eq $thumbprint -and $existing.Status -eq "Valid") {
    Write-Host "[GitManager] Already signed and verified: $resolved"
    exit 0
}

& $signtool sign /sha1 $thumbprint /fd SHA256 /tr "https://timestamp.digicert.com" /td SHA256 /d "GitManager" /du "https://github.com/Kate-alt-69/GitManager" $resolved
if ($LASTEXITCODE -ne 0) {
    throw "SignTool failed to sign $resolved (exit $LASTEXITCODE)."
}
& $signtool verify /pa /v $resolved
if ($LASTEXITCODE -ne 0) {
    throw "SignTool created a signature but Windows could not verify its trust chain for $resolved."
}
Write-Host "[GitManager] Signed and verified: $resolved"
