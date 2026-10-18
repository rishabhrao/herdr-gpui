# Inno Setup's sign tool for the release installer: `windows-installer.py build
# --sign` points ISCC at this script, which calls it once for the uninstaller it
# embeds and once for Setup.exe. It signs through Azure Artifact Signing with
# the ArtifactSigning module and Azure CLI login that the release workflow's
# windows-sign job set up before the build, with the same account, profile,
# digest and RFC 3161 timestamp as the azure/artifact-signing-action step.
param([Parameter(Mandatory)][string]$Path)

$ErrorActionPreference = 'Stop'

$settings = @{
    Endpoint               = $env:ARTIFACT_SIGNING_ENDPOINT
    CodeSigningAccountName = $env:ARTIFACT_SIGNING_ACCOUNT
    CertificateProfileName = $env:ARTIFACT_SIGNING_PROFILE
}
foreach ($name in $settings.Keys) {
    if ([string]::IsNullOrWhiteSpace($settings[$name])) {
        throw "Artifact Signing $name is not configured"
    }
}

# Splatted like the action does, so switch and boolean parameters both bind.
# Only the Azure CLI login from OIDC may authenticate, never a stray credential.
$options = @{
    Files                               = $Path
    FileDigest                          = 'SHA256'
    TimestampRfc3161                    = 'http://timestamp.acs.microsoft.com'
    TimestampDigest                     = 'SHA256'
    ExcludeAzureCliCredential           = $false
    ExcludeAzureDeveloperCliCredential  = $true
    ExcludeAzurePowerShellCredential    = $true
    ExcludeEnvironmentCredential        = $true
    ExcludeInteractiveBrowserCredential = $true
    ExcludeManagedIdentityCredential    = $true
    ExcludeSharedTokenCacheCredential   = $true
    ExcludeVisualStudioCodeCredential   = $true
    ExcludeVisualStudioCredential       = $true
    ExcludeWorkloadIdentityCredential   = $true
}
Invoke-ArtifactSigning @settings @options

$signature = Get-AuthenticodeSignature -LiteralPath $Path
if ($signature.Status -ne 'Valid' -or $null -eq $signature.TimeStamperCertificate) {
    throw "$Path is not validly signed with a timestamp: $($signature.Status)"
}
