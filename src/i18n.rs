#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    ZhTw,
    En,
}

impl Lang {
    pub fn toggle(self) -> Self {
        match self {
            Lang::ZhTw => Lang::En,
            Lang::En => Lang::ZhTw,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::ZhTw => "繁中 / EN",
            Lang::En => "EN / 繁中",
        }
    }
}

pub struct Strings {
    pub lang: Lang,
}

impl Strings {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        match self.lang {
            Lang::ZhTw => zh,
            Lang::En => en,
        }
    }

    pub fn app_subtitle(&self) -> &'static str {
        self.t("輕量 GIF 裁切與縮放工具", "Lightweight GIF Crop & Scale")
    }

    pub fn open_gif(&self) -> &'static str {
        self.t("開啟 GIF", "Open GIF")
    }

    pub fn export_gif(&self) -> &'static str {
        self.t("匯出 GIF", "Export GIF")
    }

    pub fn exporting(&self) -> &'static str {
        self.t("匯出中...", "Exporting...")
    }

    pub fn cancel(&self) -> &'static str {
        self.t("取消", "Cancel")
    }

    pub fn drop_hint_title(&self) -> &'static str {
        self.t("拖放 GIF 檔案至此處", "Drop a GIF file here")
    }

    pub fn drop_hint_sub(&self) -> &'static str {
        self.t(
            "或點擊上方「開啟 GIF」按鈕 (Ctrl+O)",
            "Or click \"Open GIF\" above (Ctrl+O)",
        )
    }

    pub fn drop_overlay(&self) -> &'static str {
        self.t("放開以載入 GIF", "Release to load GIF")
    }

    // View modes
    pub fn view_crop(&self) -> &'static str {
        self.t("裁切視圖", "Crop View")
    }

    pub fn view_preview(&self) -> &'static str {
        self.t("即時預覽", "Live Preview")
    }

    pub fn view_compare(&self) -> &'static str {
        self.t("並排對比", "Compare")
    }

    pub fn compare_original(&self) -> &'static str {
        self.t("裁切原圖", "Cropped Original")
    }

    pub fn compare_resized(&self) -> &'static str {
        self.t("縮放結果", "Resized Output")
    }

    pub fn zoom_label(&self) -> &'static str {
        self.t("檢視倍率", "Zoom")
    }

    pub fn zoom_fit(&self) -> &'static str {
        self.t("適合視窗", "Fit")
    }

    // Spatial Crop section
    pub fn section_crop(&self) -> &'static str {
        self.t("空間裁切 (Crop)", "Spatial Crop")
    }

    pub fn width(&self) -> &'static str {
        self.t("寬度", "Width")
    }

    pub fn height(&self) -> &'static str {
        self.t("高度", "Height")
    }

    pub fn aspect_ratio(&self) -> &'static str {
        self.t("裁切比例", "Aspect Ratio")
    }

    pub fn ratio_free(&self) -> &'static str {
        self.t("自由", "Free")
    }

    pub fn ratio_original(&self) -> &'static str {
        self.t("原比例", "Orig")
    }

    pub fn show_grid(&self) -> &'static str {
        self.t("顯示九宮格輔助線", "Rule-of-thirds grid")
    }

    pub fn reset_crop(&self) -> &'static str {
        self.t("重置裁切", "Reset Crop")
    }

    // Resize section
    pub fn section_resize(&self) -> &'static str {
        self.t("縮放與演算法 (Scale)", "Resize & Algorithm")
    }

    pub fn algorithm(&self) -> &'static str {
        self.t("縮放演算法", "Algorithm")
    }

    pub fn lock_aspect(&self) -> &'static str {
        self.t("鎖定長寬比", "Lock aspect ratio")
    }

    pub fn quick_scale(&self) -> &'static str {
        self.t("快速縮放", "Quick Scale")
    }

    pub fn algo_nearest_name(&self) -> &'static str {
        self.t("Nearest (鄰近像素)", "Nearest Neighbor")
    }

    pub fn algo_nearest_desc(&self) -> &'static str {
        self.t(
            "保留銳利像素邊緣，最適合像素風 / Pixel Art",
            "Crisp hard edges, ideal for pixel art",
        )
    }

    pub fn algo_box_name(&self) -> &'static str {
        self.t("Box (盒狀平均)", "Box Filter")
    }

    pub fn algo_box_desc(&self) -> &'static str {
        self.t(
            "區域像素平均，適合大幅縮小且減少鋸齒",
            "Area averaging, great for clean downscaling",
        )
    }

    pub fn algo_bilinear_name(&self) -> &'static str {
        self.t("Bilinear (雙線性)", "Bilinear")
    }

    pub fn algo_bilinear_desc(&self) -> &'static str {
        self.t(
            "快速平滑過渡，適合一般即時縮放",
            "Fast smooth interpolation for general resizing",
        )
    }

    pub fn algo_catmull_name(&self) -> &'static str {
        self.t("Bicubic Catmull-Rom (雙立方銳利)", "Bicubic (Catmull-Rom)")
    }

    pub fn algo_catmull_desc(&self) -> &'static str {
        self.t(
            "清晰且自然的雙立方縮放，細節表現均衡",
            "Sharp, natural bicubic with balanced detail",
        )
    }

    pub fn algo_mitchell_name(&self) -> &'static str {
        self.t("Bicubic Mitchell (雙立方柔和)", "Bicubic (Mitchell)")
    }

    pub fn algo_mitchell_desc(&self) -> &'static str {
        self.t(
            "柔和且低振鈴失真，適合動畫與文字畫面",
            "Smooth bicubic with minimal ringing artifacts",
        )
    }

    pub fn algo_lanczos3_name(&self) -> &'static str {
        self.t("Lanczos3 (蘭索斯高銳利)", "Lanczos3")
    }

    pub fn algo_lanczos3_desc(&self) -> &'static str {
        self.t(
            "最高解析度與邊緣銳利度，適合高品質縮小",
            "Highest sharpness and detail preservation",
        )
    }

    // Export section
    pub fn section_export(&self) -> &'static str {
        self.t("匯出設定 (Export)", "Export Settings")
    }

    pub fn quality(&self) -> &'static str {
        self.t("調色盤品質", "Palette Quality")
    }

    pub fn quality_high(&self) -> &'static str {
        self.t("高畫質 (較慢)", "High (Slower)")
    }

    pub fn quality_balanced(&self) -> &'static str {
        self.t("平衡 (推薦)", "Balanced (Recommended)")
    }

    pub fn quality_fast(&self) -> &'static str {
        self.t("快速 (較快)", "Fast")
    }

    pub fn speed_label(&self) -> &'static str {
        self.t("播放/輸出倍速", "Playback Speed")
    }

    pub fn apply_speed_export(&self) -> &'static str {
        self.t("將倍速套用至匯出 GIF", "Apply speed to exported GIF")
    }

    pub fn output_summary(&self) -> &'static str {
        self.t("輸出摘要", "Output Summary")
    }

    pub fn frames_unit(&self) -> &'static str {
        self.t("幀", "frames")
    }

    // Timeline section
    pub fn timeline_in(&self) -> &'static str {
        self.t("起點", "In")
    }

    pub fn timeline_out(&self) -> &'static str {
        self.t("終點", "Out")
    }

    pub fn set_in(&self) -> &'static str {
        self.t("設為起點 [I]", "Set In [I]")
    }

    pub fn set_out(&self) -> &'static str {
        self.t("設為終點 [O]", "Set Out [O]")
    }

    pub fn reset_trim(&self) -> &'static str {
        self.t("重置區間", "Reset Trim")
    }

    pub fn frame_label(&self) -> &'static str {
        self.t("幀", "Frame")
    }

    pub fn shortcuts_hint(&self) -> &'static str {
        self.t(
            "Space 播放/暫停 · ←/→ 逐幀 · I/O 設起訖點",
            "Space Play/Pause · ←/→ Step · I/O Trim In/Out",
        )
    }

    // Status messages
    pub fn status_ready(&self) -> &'static str {
        self.t("就緒", "Ready")
    }

    pub fn status_loaded(&self) -> &'static str {
        self.t("已載入", "Loaded")
    }

    pub fn status_export_done(&self) -> &'static str {
        self.t("匯出成功", "Exported successfully")
    }

    pub fn status_export_cancelled(&self) -> &'static str {
        self.t("已取消匯出", "Export cancelled")
    }

    pub fn status_error_prefix(&self) -> &'static str {
        self.t("錯誤：", "Error: ")
    }

    pub fn dialog_open_title(&self) -> &'static str {
        self.t("選擇 GIF 檔案", "Open GIF File")
    }

    pub fn dialog_save_title(&self) -> &'static str {
        self.t("儲存裁切後的 GIF", "Save Cropped GIF")
    }
}
