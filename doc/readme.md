
Invoke-WebRequest -Uri "http://archive.ubuntu.com/ubuntu/pool/main/g/gcc-13/gcc-13_13.2.0-4ubuntu3_amd64.deb" -OutFile "gcc-13_13.2.0-4ubuntu3_amd64.deb"

---
$outDir = ".\rust-offline-debs"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

Get-Content urls.txt | ForEach-Object {
    $fileName = Split-Path $_ -Leaf
    Invoke-WebRequest -Uri $_ -OutFile (Join-Path $outDir $fileName)
}
---
