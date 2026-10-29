param(
    [string]$Dll = "",
    [switch]$Unregister
)

$ErrorActionPreference = "Stop"

$clsid = "{a3f6d8e2-5b4c-4f1a-9d7e-2c8b1f4e6a90}"
$thumbHandlerGuid = "{E357FCCD-A995-4576-B01F-234630154E96}"
$extensions = @(".zip", ".cbz", ".epub", ".7z", ".rar", ".pdf")

$classes = "HKCU:\Software\Classes"
$clsidKey = Join-Path $classes "CLSID\$clsid"

if ($Unregister) {
    foreach ($ext in $extensions) {
        $key = Join-Path $classes "$ext\ShellEx\$thumbHandlerGuid"
        if (Test-Path $key) {
            Remove-Item -Path $key -Recurse -Force
        }
    }
    if (Test-Path $clsidKey) {
        Remove-Item -Path $clsidKey -Recurse -Force
    }
    Write-Host "Ogle: регистрация удалена."
    exit 0
}

if (-not $Dll) {
    $Dll = Join-Path $env:LOCALAPPDATA "ogle-target\release\ogle_shell.dll"
}
if (-not (Test-Path $Dll)) {
    throw "Не найден ogle_shell.dll: $Dll. Соберите проект (cargo build --release) или укажите -Dll <путь>."
}
$Dll = (Resolve-Path $Dll).Path

if ((Get-Item $Dll).Length -eq 0) {
    throw "Файл $Dll пуст. Пересоберите проект."
}

New-Item -Path $clsidKey -Force | Out-Null
Set-ItemProperty -Path $clsidKey -Name "(default)" -Value "Ogle thumbnail provider"
New-Item -Path "$clsidKey\InprocServer32" -Force | Out-Null
Set-ItemProperty -Path "$clsidKey\InprocServer32" -Name "(default)" -Value $Dll
Set-ItemProperty -Path "$clsidKey\InprocServer32" -Name "ThreadingModel" -Value "Apartment"

foreach ($ext in $extensions) {
    $key = Join-Path $classes "$ext\ShellEx\$thumbHandlerGuid"
    New-Item -Path $key -Force | Out-Null
    Set-ItemProperty -Path $key -Name "(default)" -Value $clsid
}

Write-Host "Ogle: зарегистрирован для $($extensions -join ', ')"
Write-Host ""
Write-Host "Дальше:"
Write-Host "  1. Перезапустите Explorer (или войдите заново), чтобы он подхватил DLL."
Write-Host "  2. Сбросьте кэш миниатюр: ie4uinit.exe -show"
Write-Host "  3. Отключить: powershell -File scripts\register.ps1 -Unregister"
