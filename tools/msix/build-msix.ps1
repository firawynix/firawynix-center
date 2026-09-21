[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Config,

    [Parameter(Mandatory = $true)]
    [ValidateSet('Store', 'Site')]
    [string]$Channel,

    [Parameter(Mandatory = $true)]
    [string]$Publisher,

    [string]$PayloadPath,
    [string]$OutputDirectory,
    [string]$CertificateThumbprint,
    [string]$PackageUri
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Resolve-SdkTool([string]$Name) {
    $sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $tool = Get-ChildItem -LiteralPath $sdkBin -Filter $Name -Recurse -File |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $tool) {
        throw "$Name não foi encontrado. Instale o Windows SDK."
    }
    return $tool.FullName
}

function ConvertTo-MsixVersion([string]$Version) {
    $parts = @($Version.Split('.'))
    if ($parts.Count -gt 4) {
        throw "Versão MSIX inválida: $Version"
    }
    while ($parts.Count -lt 4) { $parts += '0' }
    foreach ($part in $parts) {
        $number = 0
        if (-not [int]::TryParse($part, [ref]$number) -or $number -lt 0 -or $number -gt 65535) {
            throw "Versão MSIX inválida: $Version"
        }
    }
    return ($parts -join '.')
}

function Escape-Xml([string]$Value) {
    return [System.Security.SecurityElement]::Escape($Value)
}

function New-Logo([string]$Source, [string]$Destination, [int]$Size) {
    Add-Type -AssemblyName System.Drawing
    $icon = $null
    if ([System.IO.Path]::GetExtension($Source) -in @('.exe', '.ico')) {
        $icon = if ([System.IO.Path]::GetExtension($Source) -eq '.exe') {
            [System.Drawing.Icon]::ExtractAssociatedIcon($Source)
        } else {
            New-Object System.Drawing.Icon $Source
        }
        $sourceImage = $icon.ToBitmap()
    } else {
        $sourceImage = [System.Drawing.Image]::FromFile($Source)
    }
    try {
        $canvas = New-Object System.Drawing.Bitmap $Size, $Size
        try {
            $graphics = [System.Drawing.Graphics]::FromImage($canvas)
            try {
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $scale = [Math]::Min($Size / $sourceImage.Width, $Size / $sourceImage.Height)
                $width = [int]($sourceImage.Width * $scale)
                $height = [int]($sourceImage.Height * $scale)
                $left = [int](($Size - $width) / 2)
                $top = [int](($Size - $height) / 2)
                $graphics.DrawImage($sourceImage, $left, $top, $width, $height)
                $canvas.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
            } finally {
                $graphics.Dispose()
            }
        } finally {
            $canvas.Dispose()
        }
    } finally {
        $sourceImage.Dispose()
        if ($icon) { $icon.Dispose() }
    }
}

$configPath = (Resolve-Path -LiteralPath $Config).Path
$configDirectory = Split-Path -Parent $configPath
$settings = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json

$payload = $null
if ($PayloadPath) {
    $payload = (Resolve-Path -LiteralPath $PayloadPath).Path
} elseif ($settings.PSObject.Properties.Name -contains 'payloadPath' -and $settings.payloadPath) {
    $payload = (Resolve-Path -LiteralPath (Join-Path $configDirectory $settings.payloadPath)).Path
} elseif (-not ($settings.PSObject.Properties.Name -contains 'payloadFiles')) {
    throw 'Configure payloadPath ou payloadFiles.'
}
$icon = (Resolve-Path -LiteralPath (Join-Path $configDirectory $settings.icon)).Path

if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $configDirectory 'artifacts'
}
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path

$version = ConvertTo-MsixVersion $settings.version
if ($Channel -eq 'Store' -and [int]$version.Split('.')[3] -ne 0) {
    throw "A Microsoft Store exige revisão zero no quarto campo da versão MSIX: $version"
}
$executable = $settings.executable -replace '/', '\'
$channelName = $Channel.ToLowerInvariant()
$safeName = ($settings.displayName -replace '[^A-Za-z0-9._-]', '-') -replace '-+', '-'
$fileName = "$safeName-$version-$($settings.architecture)-$channelName.msix"
$packagePath = Join-Path $OutputDirectory $fileName
$workRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("firaw-msix-" + [Guid]::NewGuid().ToString('N'))
$stage = Join-Path $workRoot 'package'
$appDirectory = Join-Path $stage 'app'
$assetsDirectory = Join-Path $stage 'Assets'

try {
    $null = New-Item -ItemType Directory -Force -Path $appDirectory, $assetsDirectory
    if ($payload) {
        if ((Get-Item -LiteralPath $payload).PSIsContainer) {
            Get-ChildItem -LiteralPath $payload -Force | Copy-Item -Destination $appDirectory -Recurse -Force
        } else {
            Copy-Item -LiteralPath $payload -Destination (Join-Path $appDirectory (Split-Path -Leaf $payload)) -Force
        }
    } else {
        foreach ($entry in $settings.payloadFiles) {
            $source = (Resolve-Path -LiteralPath (Join-Path $configDirectory $entry.source)).Path
            $destination = Join-Path $appDirectory $entry.destination
            $resolvedDestination = [System.IO.Path]::GetFullPath($destination)
            if (-not $resolvedDestination.StartsWith($appDirectory, [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "Destino fora do pacote: $($entry.destination)"
            }
            $parent = Split-Path -Parent $resolvedDestination
            $null = New-Item -ItemType Directory -Force -Path $parent
            Copy-Item -LiteralPath $source -Destination $resolvedDestination -Recurse -Force
        }
    }

    $executablePath = Join-Path $appDirectory $executable
    if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
        throw "Executável não encontrado no payload: $executablePath"
    }

    New-Logo $icon (Join-Path $assetsDirectory 'StoreLogo.png') 50
    New-Logo $icon (Join-Path $assetsDirectory 'Square44x44Logo.png') 44
    New-Logo $icon (Join-Path $assetsDirectory 'Square150x150Logo.png') 150

    $identityName = if ($Channel -eq 'Store') { $settings.storeIdentityName } else { $settings.siteIdentityName }
    if (-not $identityName -or $identityName -like 'REPLACE_*') {
        throw "A identidade do canal $Channel não foi configurada."
    }

    $extraProperties = ''
    $extraCapabilities = ''
    if (($settings.PSObject.Properties.Name -contains 'unvirtualizedRegistry') -and $settings.unvirtualizedRegistry) {
        $extraProperties = "`n    <desktop6:RegistryWriteVirtualization>disabled</desktop6:RegistryWriteVirtualization>"
        $extraCapabilities = "`n    <rescap:Capability Name=`"unvirtualizedResources`" />"
    }

    $extensionItems = New-Object System.Collections.Generic.List[string]
    if ($settings.PSObject.Properties.Name -contains 'protocols') {
        foreach ($protocol in $settings.protocols) {
            $parameters = if (($protocol.PSObject.Properties.Name -contains 'parameters') -and $protocol.parameters) {
                [string]$protocol.parameters
            } else {
                '"%1"'
            }
            $extensionItems.Add(@"
      <uap3:Extension Category="windows.protocol">
        <uap3:Protocol Name="$(Escape-Xml ([string]$protocol.name))" Parameters="$(Escape-Xml $parameters)" />
      </uap3:Extension>
"@)
        }
    }
    if (($settings.PSObject.Properties.Name -contains 'fileTypes') -and $settings.fileTypes.Count -gt 0) {
        $fileTypeItems = ($settings.fileTypes | ForEach-Object { "          <uap:FileType>$(Escape-Xml ([string]$_))</uap:FileType>" }) -join "`n"
        $extensionItems.Add(@"
      <uap:Extension Category="windows.fileTypeAssociation">
        <uap3:FileTypeAssociation Name="webdocuments" Parameters="&quot;%1&quot;">
          <uap:SupportedFileTypes>
$fileTypeItems
          </uap:SupportedFileTypes>
        </uap3:FileTypeAssociation>
      </uap:Extension>
"@)
    }
    $applicationExtensions = if ($extensionItems.Count -gt 0) {
        "`n    <Extensions>`n" + ($extensionItems -join "`n") + "    </Extensions>"
    } else { '' }

    $manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap3="http://schemas.microsoft.com/appx/manifest/uap/windows10/3"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:desktop6="http://schemas.microsoft.com/appx/manifest/desktop/windows10/6"
  xmlns:virtualization="http://schemas.microsoft.com/appx/manifest/virtualization/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap uap3 uap10 desktop6 virtualization rescap">
  <Identity Name="$(Escape-Xml $identityName)" Publisher="$(Escape-Xml $Publisher)" Version="$version" ProcessorArchitecture="$($settings.architecture)" />
  <Properties>
    <DisplayName>$(Escape-Xml $settings.displayName)</DisplayName>
    <PublisherDisplayName>$(Escape-Xml $settings.publisherDisplayName)</PublisherDisplayName>
    <Description>$(Escape-Xml $settings.description)</Description>
    <Logo>Assets\StoreLogo.png</Logo>$extraProperties
  </Properties>
  <Resources>
    <Resource Language="pt-BR" />
    <Resource Language="en-US" />
  </Resources>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Applications>
    <Application Id="$($settings.applicationId)" Executable="app\$executable" uap10:RuntimeBehavior="packagedClassicApp" uap10:TrustLevel="mediumIL">
      <uap:VisualElements DisplayName="$(Escape-Xml $settings.displayName)" Description="$(Escape-Xml $settings.description)" BackgroundColor="transparent" Square44x44Logo="Assets\Square44x44Logo.png" Square150x150Logo="Assets\Square150x150Logo.png" />$applicationExtensions
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />$extraCapabilities
  </Capabilities>
</Package>
"@
    [System.IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, (New-Object System.Text.UTF8Encoding($false)))

    $makeAppx = Resolve-SdkTool 'makeappx.exe'
    & $makeAppx pack /o /d $stage /p $packagePath
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx falhou com código $LASTEXITCODE." }

    $signed = $false
    if ($CertificateThumbprint) {
        $certificate = Get-ChildItem -Path Cert:\CurrentUser\My\$CertificateThumbprint -ErrorAction Stop
        if ($certificate.Subject -ne $Publisher) {
            throw "O Publisher '$Publisher' não corresponde ao certificado '$($certificate.Subject)'."
        }
        $signTool = Resolve-SdkTool 'signtool.exe'
        $timestampServers = @(
            'http://timestamp.acs.microsoft.com',
            'http://timestamp.digicert.com'
        )
        foreach ($timestampServer in $timestampServers) {
            & $signTool sign /sha1 $CertificateThumbprint /fd SHA256 /td SHA256 /tr $timestampServer $packagePath
            if ($LASTEXITCODE -eq 0) {
                $signed = $true
                break
            }
            Write-Warning "O carimbo de tempo falhou em $timestampServer; tentando o próximo servidor."
        }
        if (-not $signed) { throw 'SignTool não conseguiu assinar o pacote com carimbo de tempo.' }
    }

    if ($PackageUri) {
        if ($Channel -ne 'Site') { throw 'PackageUri só é válido no canal Site.' }
        if (-not $signed) { throw 'O .appinstaller só pode ser gerado depois que o MSIX estiver assinado.' }
        $appInstallerPath = [System.IO.Path]::ChangeExtension($packagePath, '.appinstaller')
        $appInstallerUri = [Uri]$PackageUri
        $appInstaller = @"
<?xml version="1.0" encoding="utf-8"?>
<AppInstaller xmlns="http://schemas.microsoft.com/appx/appinstaller/2018" Version="$version" Uri="$(Escape-Xml ([System.IO.Path]::ChangeExtension($PackageUri, '.appinstaller')))" >
  <MainPackage Name="$(Escape-Xml $identityName)" Publisher="$(Escape-Xml $Publisher)" Version="$version" ProcessorArchitecture="$($settings.architecture)" Uri="$(Escape-Xml $appInstallerUri.AbsoluteUri)" />
  <UpdateSettings>
    <OnLaunch HoursBetweenUpdateChecks="0" ShowPrompt="true" UpdateBlocksActivation="false" />
    <AutomaticBackgroundTask />
  </UpdateSettings>
</AppInstaller>
"@
        [System.IO.File]::WriteAllText($appInstallerPath, $appInstaller, (New-Object System.Text.UTF8Encoding($false)))
    }

    $hash = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLowerInvariant()
    [pscustomobject]@{
        package = $packagePath
        channel = $Channel
        identity = $identityName
        publisher = $Publisher
        version = $version
        signed = $signed
        sha256 = $hash
    } | ConvertTo-Json
} finally {
    if (Test-Path -LiteralPath $workRoot) {
        Remove-Item -LiteralPath $workRoot -Recurse -Force
    }
}
