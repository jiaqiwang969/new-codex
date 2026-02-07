use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;

/// State for batch image processing via `/ref-image-batch` and `/pdf-update`.
pub(crate) struct BatchImageState {
    pub source_dir: PathBuf,
    pending_images: VecDeque<PathBuf>,
    pub prompt: String,
    total_count: usize,
    processed_count: usize,
    pub current_image: Option<PathBuf>,
    pub original_pdf_path: Option<PathBuf>,
}

impl BatchImageState {
    pub fn new(source_dir: PathBuf, images: Vec<PathBuf>, prompt: String) -> Self {
        let total_count = images.len();
        Self {
            source_dir,
            pending_images: images.into(),
            prompt,
            total_count,
            processed_count: 0,
            current_image: None,
            original_pdf_path: None,
        }
    }

    pub fn new_for_pdf(
        source_dir: PathBuf,
        images: Vec<PathBuf>,
        prompt: String,
        pdf_path: PathBuf,
    ) -> Self {
        let total_count = images.len();
        Self {
            source_dir,
            pending_images: images.into(),
            prompt,
            total_count,
            processed_count: 0,
            current_image: None,
            original_pdf_path: Some(pdf_path),
        }
    }

    pub fn next_image(&mut self) -> Option<PathBuf> {
        let next = self.pending_images.pop_front()?;
        self.current_image = Some(next.clone());
        Some(next)
    }

    pub fn mark_current_processed(&mut self) {
        if self.current_image.is_some() {
            self.processed_count += 1;
            self.current_image = None;
        }
    }

    pub fn progress_message(&self) -> String {
        let name = self
            .current_image
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        format!(
            "[Batch] Processing {}/{}: {}",
            self.processed_count + 1,
            self.total_count,
            name
        )
    }

    pub fn completion_message(&self) -> String {
        let dir_name = self
            .source_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        format!(
            "[Batch] Complete! Processed {} images in {}",
            self.processed_count, dir_name
        )
    }
}

/// State for pending PDF update operation via `/pdf-update`.
pub(crate) struct PendingPdfUpdate {
    pub pdf_path: PathBuf,
    pub images_output_dir: PathBuf,
    pub prompt: String,
}

/// Resolve a user-provided path (supports `~/`, absolute, and relative).
pub(crate) fn resolve_user_path(raw: &str, cwd: &Path) -> PathBuf {
    if raw.starts_with('/') {
        PathBuf::from(raw)
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(stripped)
        } else {
            cwd.join(raw)
        }
    } else {
        cwd.join(raw)
    }
}

/// Scan a directory for image files, skipping already-processed ones.
pub(crate) fn scan_image_files(dir: &Path) -> Vec<PathBuf> {
    let image_extensions = ["png", "jpg", "jpeg", "webp", "gif"];
    let mut images: Vec<PathBuf> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    let is_processed = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.ends_with("_processed"))
                        .unwrap_or(false);
                    if !is_processed && image_extensions.contains(&ext_lower.as_str()) {
                        images.push(path);
                    }
                }
            }
        }
    }

    images
}

/// Embedded Python script for PDF-to-image conversion and watermark removal.
pub(crate) const PDF_PROCESS_SCRIPT: &str = r#"
import sys
import os
from pathlib import Path

def main():
    if len(sys.argv) < 3:
        print("Usage: python script.py <input_pdf> <output_dir> [dpi]", file=sys.stderr)
        sys.exit(1)

    input_pdf = sys.argv[1]
    output_dir = sys.argv[2]
    dpi = int(sys.argv[3]) if len(sys.argv) > 3 else 200

    if not os.path.exists(input_pdf):
        print(f"Error: Input PDF not found: {input_pdf}", file=sys.stderr)
        sys.exit(1)

    try:
        from pdf2image import convert_from_path
        import cv2
        import numpy as np
    except ImportError as e:
        print(f"Error: Missing dependency: {e}", file=sys.stderr)
        print("Run: pip install pdf2image opencv-python-headless numpy", file=sys.stderr)
        sys.exit(1)

    Path(output_dir).mkdir(parents=True, exist_ok=True)

    print(f"Converting PDF to images (DPI={dpi})...")
    try:
        images = convert_from_path(input_pdf, dpi=dpi)
    except Exception as e:
        print(f"Error converting PDF: {e}", file=sys.stderr)
        print("Make sure poppler is installed (brew install poppler)", file=sys.stderr)
        sys.exit(1)

    print(f"Total pages: {len(images)}")
    print("Removing watermarks...")
    processed_count = 0

    for i, image in enumerate(images):
        temp_path = os.path.join(output_dir, f"_temp_{i}.png")
        output_path = os.path.join(output_dir, f"page_{i+1:03d}.png")
        image.save(temp_path, "PNG")

        img = cv2.imread(temp_path)
        height, width = img.shape[:2]
        roi_x, roi_y = int(width * 0.80), int(height * 0.92)
        roi = img[roi_y:height, roi_x:width]
        gray_roi = cv2.cvtColor(roi, cv2.COLOR_BGR2GRAY)
        mask_roi = cv2.inRange(gray_roi, 150, 240)
        kernel = cv2.getStructuringElement(cv2.MORPH_RECT, (5, 5))
        mask_roi = cv2.dilate(mask_roi, kernel, iterations=2)
        mask = np.zeros((height, width), dtype=np.uint8)
        mask[roi_y:height, roi_x:width] = mask_roi

        if np.sum(mask) > 100:
            kernel_expand = cv2.getStructuringElement(cv2.MORPH_RECT, (7, 7))
            mask = cv2.dilate(mask, kernel_expand, iterations=1)
            result = cv2.inpaint(img, mask, inpaintRadius=5, flags=cv2.INPAINT_TELEA)
            cv2.imwrite(output_path, result)
            processed_count += 1
            print(f"  page_{i+1:03d}.png: watermark removed")
        else:
            cv2.imwrite(output_path, img)
            print(f"  page_{i+1:03d}.png: no watermark")
        os.remove(temp_path)

    print(f"Done! {len(images)} pages, {processed_count} watermarks removed")

if __name__ == "__main__":
    main()
"#;

/// Embedded Python script for merging processed images into a PPTX.
pub(crate) const MERGE_PPTX_SCRIPT: &str = r#"
import sys
import os

def main():
    if len(sys.argv) < 3:
        print("Usage: python script.py <image_dir> <output_pptx>", file=sys.stderr)
        sys.exit(1)

    image_dir = sys.argv[1]
    output_pptx = sys.argv[2]

    try:
        from pptx import Presentation
        from pptx.util import Inches
        from PIL import Image
    except ImportError as e:
        print(f"Error: Missing dependency: {e}", file=sys.stderr)
        print("Run: pip install python-pptx Pillow", file=sys.stderr)
        sys.exit(1)

    images = sorted([
        os.path.join(image_dir, f) for f in os.listdir(image_dir)
        if f.endswith('_processed.png')
    ])

    if not images:
        images = sorted([
            os.path.join(image_dir, f) for f in os.listdir(image_dir)
            if f.lower().endswith('.png') and not f.startswith('_temp_')
        ])

    if not images:
        print("Error: No images found to merge", file=sys.stderr)
        sys.exit(1)

    print(f"Found {len(images)} images")

    prs = Presentation()

    with Image.open(images[0]) as img:
        img_width, img_height = img.size

    slide_width = Inches(10)
    slide_height = Inches(10 * img_height / img_width)
    prs.slide_width = int(slide_width)
    prs.slide_height = int(slide_height)

    blank_layout = prs.slide_layouts[6]

    for img_path in images:
        slide = prs.slides.add_slide(blank_layout)
        slide.shapes.add_picture(
            img_path,
            Inches(0),
            Inches(0),
            width=int(slide_width),
            height=int(slide_height)
        )
        print(f"  Added: {os.path.basename(img_path)}")

    prs.save(output_pptx)
    print(f"PPTX created: {output_pptx}")

if __name__ == "__main__":
    main()
"#;
