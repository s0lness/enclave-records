# WINDOWS ONLY. Forwards USB into WSL, which is a problem that exists only on
# Windows: on Linux or macOS a Ledger is just a USB device and nothing here is
# needed. Skip this whole directory.
#
# Attach the connected Ledger Flex devices to WSL (run from Windows PowerShell).
# Prereq (one-time, admin): winget install dorssel.usbipd-win
# First time per device (admin): usbipd bind --busid <ID>
# Ledger USB VID is 2c97.
$devices = usbipd list | Select-String "2c97"
if (-not $devices) {
    Write-Host "No Ledger device found. Plug the Flex(es) in and unlock them."
    exit 1
}
$devices | ForEach-Object {
    $busid = ($_ -split "\s+")[0]
    Write-Host "Attaching $busid to WSL..."
    usbipd attach --wsl --busid $busid
}
usbipd list | Select-String "2c97"
