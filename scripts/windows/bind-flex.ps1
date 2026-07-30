# WINDOWS ONLY (see attach-usb.ps1): usbipd exists to hand a USB device to WSL.
# Nothing in this directory has a Linux or macOS equivalent, because there the
# device is already visible.
#
# Elevated helper: share every connected Ledger device with WSL (usbipd bind).
$usbipd = "C:\Program Files\usbipd-win\usbipd.exe"
$log = Join-Path $env:TEMP "bind-flex.log"
"start $(Get-Date)" | Out-File $log
& $usbipd list | Select-String "2c97" | ForEach-Object {
    $busid = ($_ -split "\s+")[0]
    "binding $busid" | Out-File $log -Append
    & $usbipd bind --busid $busid 2>&1 | Out-File $log -Append
}
& $usbipd list | Out-File $log -Append
