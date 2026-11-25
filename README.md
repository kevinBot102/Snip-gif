# Snip

輕量、免安裝的 Windows 原生 GIF 裁切、即時預覽與多演算法縮放工具（繁體中文 / English 雙語介面）。  
A lightweight, portable native Windows GIF cropper, real-time previewer, and multi-algorithm scaler (Traditional Chinese / English UI).

## 使用方式 / Quick Start

- 直接雙擊執行 `Snip.exe`（單檔免安裝，不需額外執行時期環境）。
- 將 `.gif` 檔案拖放至視窗內（或直接拖放到 `Snip.exe` 圖示上），或點擊「開啟 GIF」(`Ctrl+O`)。

## 主要功能 / Features

- **空間裁切 (Spatial Crop)**
  - 直接在畫面上拖曳 8 個控制點縮放裁切框、拖曳框內移動位置，或在框外拖曳重新框選。
  - 支援數值微調 (`X, Y, W, H`)、自由比例 / `1:1` / `16:9` / `4:3` / `3:2` / 原比例預設、九宮格輔助線。
- **時間軸裁切 (Timeline Trim)**
  - 視覺化時間軸拖曳起點 (`In`)、終點 (`Out`) 與播放頭。
  - 支援播放/暫停 (`Space`)、逐幀前後檢視 (`←` / `→`)、快捷設點 (`I` / `O`)、倍速預覽與匯出倍速調整。
- **即時預覽與並排對比 (Real-Time Preview & Compare)**
  - **裁切視圖 / Crop View**：即時播放動畫並調整裁切區域。
  - **即時預覽 / Live Preview**：即時呈現裁切＋演算法縮放後的動態畫面，支援 `Fit / 50% / 100% / 200% / 400%` 像素級檢視與透明棋盤格背景。
  - **並排對比 / Compare**：左右同步比較「裁切原圖」與「縮放結果」，即時觀察各演算法差異。
- **多種縮放演算法 (SIMD 加速)**
  - `Nearest Neighbor`（鄰近像素，適合 Pixel Art）
  - `Box`（盒狀平均，適合大幅縮小）
  - `Bilinear`（雙線性，快速平滑）
  - `Bicubic (Catmull-Rom)`（雙立方銳利）
  - `Bicubic (Mitchell)`（雙立方柔和、低振鈴）
  - `Lanczos3`（蘭索斯，最高細節銳利度）
- **雙語介面 (Bilingual UI)**
  - 右上角一鍵即時切換 **繁體中文 / English**。

## 自行編譯 / Build from Source

```powershell
cargo build --release
```
