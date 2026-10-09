# 下载 AI 蒙版模型到指定目录（默认 exe/dist 下的 models/）。
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File packaging/windows/download-models.ps1
#   powershell -ExecutionPolicy Bypass -File packaging/windows/download-models.ps1 -TargetDir "F:\FD-Room\dist\windows\models"
#
# 说明：全部走 hf-mirror.com 镜像（GitHub / HuggingFace 直连不可用）。
param(
    [string]$TargetDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "dist/windows/models")
)

$ErrorActionPreference = "Stop"
$ua = @{ "User-Agent" = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0 Safari/537.36" }
New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null

$models = @(
    @{ name = "birefnet_lite.onnx"; url = "https://hf-mirror.com/onnx-community/BiRefNet_lite-ONNX/resolve/main/onnx/model.onnx" },
    @{ name = "face_parsing_quantized.onnx"; url = "https://hf-mirror.com/Xenova/face-parsing/resolve/main/onnx/model_quantized.onnx" },
    @{ name = "segformer_ade_b0.onnx"; url = "https://hf-mirror.com/lquint/segformer-b0-finetuned-ade-512-512-onnx/resolve/main/onnx/model.onnx" }
)

foreach ($model in $models) {
    $target = Join-Path $TargetDir $model.name
    if (Test-Path $target) {
        Write-Host ("{0} 已存在（{1:N1} MB）" -f $model.name, ((Get-Item $target).Length / 1MB))
        continue
    }
    Write-Host ("下载 {0} …" -f $model.name)
    $temp = "$target.part"
    Invoke-WebRequest -Uri $model.url -OutFile $temp -TimeoutSec 3600 -Headers $ua
    Move-Item -LiteralPath $temp -Destination $target -Force
    Write-Host ("{0} 完成（{1:N1} MB）" -f $model.name, ((Get-Item $target).Length / 1MB))
}

Write-Host "模型目录：$TargetDir" -ForegroundColor Green

