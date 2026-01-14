use image::ImageFormat;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

// Validation constants
pub const MAX_FILES: usize = 50;
pub const MAX_SINGLE_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500MB
pub const MAX_TOTAL_SIZE: u64 = 1024 * 1024 * 1024; // 1GB

// Allowed file extensions
const ALLOWED_EXTENSIONS: &[&str] = &[
    // Images
    "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "webp", "heic", "heif",
    // Documents
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv",
    // Archives
    "zip", "tar", "gz", "7z", "rar",
    // Video
    "mov", "mp4", "avi", "mkv", "webm", "m4v",
    // Audio
    "mp3", "wav", "aac", "flac", "m4a", "ogg",
    // Code/Data
    "json", "xml", "html", "css", "js", "ts", "py", "rs", "go", "swift",
    // Other
    "svg", "ico", "dmg", "pkg", "app",
];

/// Result of processing files
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessResult {
    pub output_path: PathBuf,
    pub original_size: u64,
    pub processed_size: u64,
    pub file_type: String,
}

/// Validation error details
#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationError {
    pub message: String,
    pub file: Option<String>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Validate files before processing
pub fn validate_files(paths: &[PathBuf]) -> Result<(), ValidationError> {
    // Check file count
    if paths.is_empty() {
        return Err(ValidationError {
            message: "No files provided".to_string(),
            file: None,
        });
    }

    if paths.len() > MAX_FILES {
        return Err(ValidationError {
            message: format!("Too many files. Maximum is {} files.", MAX_FILES),
            file: None,
        });
    }

    let mut total_size: u64 = 0;

    for path in paths {
        // Check file exists
        if !path.exists() {
            return Err(ValidationError {
                message: format!("File not found: {}", path.display()),
                file: Some(path.to_string_lossy().to_string()),
            });
        }

        // Check it's a file, not a directory
        if path.is_dir() {
            return Err(ValidationError {
                message: "Directories are not supported. Please zip the folder first.".to_string(),
                file: Some(path.to_string_lossy().to_string()),
            });
        }

        // Check file size
        let metadata = fs::metadata(path).map_err(|e| ValidationError {
            message: format!("Cannot read file: {}", e),
            file: Some(path.to_string_lossy().to_string()),
        })?;

        let file_size = metadata.len();

        if file_size > MAX_SINGLE_FILE_SIZE {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            return Err(ValidationError {
                message: format!(
                    "\"{}\" is too large ({:.1} MB). Maximum file size is 500 MB.",
                    file_name,
                    file_size as f64 / (1024.0 * 1024.0)
                ),
                file: Some(path.to_string_lossy().to_string()),
            });
        }

        total_size += file_size;

        // Validate extension
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());

        if let Some(ext) = ext {
            if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                return Err(ValidationError {
                    message: format!("Unsupported file type: .{} ({})", ext, file_name),
                    file: Some(path.to_string_lossy().to_string()),
                });
            }
        }
        // Files without extensions are allowed (will be treated as binary)
    }

    // Check total size
    if total_size > MAX_TOTAL_SIZE {
        return Err(ValidationError {
            message: format!(
                "Total size ({:.1} MB) exceeds 1 GB limit.",
                total_size as f64 / (1024.0 * 1024.0)
            ),
            file: None,
        });
    }

    Ok(())
}

/// Determine if a file is an image based on extension
fn is_image(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    matches!(
        ext.as_deref(),
        Some("jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "tif")
    )
}

/// Check if a file is already WebP
fn is_webp(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
    
    ext.as_deref() == Some("webp")
}

/// Convert an image to WebP format at 80% quality
pub fn convert_to_webp(input_path: &Path, output_dir: &Path) -> Result<ProcessResult, String> {
    let original_size = fs::metadata(input_path)
        .map_err(|e| format!("Failed to read file metadata: {}", e))?
        .len();

    let img = image::open(input_path).map_err(|e| format!("Failed to open image: {}", e))?;

    // Generate output filename with unique suffix to avoid conflicts
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    
    let unique_id = &uuid::Uuid::new_v4().to_string()[..8];
    let output_path = output_dir.join(format!("{}_{}.webp", stem, unique_id));

    // Save as WebP
    let output_file =
        File::create(&output_path).map_err(|e| format!("Failed to create output file: {}", e))?;
    let mut writer = BufWriter::new(output_file);

    img.write_to(&mut writer, ImageFormat::WebP)
        .map_err(|e| format!("Failed to write WebP: {}", e))?;

    writer
        .flush()
        .map_err(|e| format!("Failed to flush: {}", e))?;

    let processed_size = fs::metadata(&output_path)
        .map_err(|e| format!("Failed to read output metadata: {}", e))?
        .len();

    Ok(ProcessResult {
        output_path,
        original_size,
        processed_size,
        file_type: "webp".to_string(),
    })
}

/// Create a zip archive from multiple files
pub fn create_zip(input_paths: &[PathBuf], output_dir: &Path) -> Result<ProcessResult, String> {
    let unique_id = &uuid::Uuid::new_v4().to_string()[..8];
    let output_path = output_dir.join(format!("archive_{}.zip", unique_id));

    let file =
        File::create(&output_path).map_err(|e| format!("Failed to create zip file: {}", e))?;
    let mut zip = ZipWriter::new(file);

    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut total_original_size: u64 = 0;

    for path in input_paths {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let file_data =
            fs::read(path).map_err(|e| format!("Failed to read file {}: {}", file_name, e))?;

        total_original_size += file_data.len() as u64;

        zip.start_file(file_name, options)
            .map_err(|e| format!("Failed to start zip entry: {}", e))?;

        zip.write_all(&file_data)
            .map_err(|e| format!("Failed to write to zip: {}", e))?;
    }

    zip.finish()
        .map_err(|e| format!("Failed to finish zip: {}", e))?;

    let processed_size = fs::metadata(&output_path)
        .map_err(|e| format!("Failed to read zip metadata: {}", e))?
        .len();

    Ok(ProcessResult {
        output_path,
        original_size: total_original_size,
        processed_size,
        file_type: "zip".to_string(),
    })
}

/// Copy a file to the output directory (for passthrough)
fn copy_file(input_path: &Path, output_dir: &Path) -> Result<ProcessResult, String> {
    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_lowercase();

    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");

    let unique_id = &uuid::Uuid::new_v4().to_string()[..8];
    let output_path = output_dir.join(format!("{}_{}.{}", stem, unique_id, ext));

    let original_size = fs::metadata(input_path)
        .map_err(|e| format!("Failed to read file metadata: {}", e))?
        .len();

    fs::copy(input_path, &output_path).map_err(|e| format!("Failed to copy file: {}", e))?;

    Ok(ProcessResult {
        output_path,
        original_size,
        processed_size: original_size,
        file_type: ext,
    })
}

/// Process files according to the ZipDrop logic:
/// - Single convertible image → WebP conversion
/// - Multiple files → ZIP archive
/// - Single non-image (or already WebP) → passthrough
pub fn process_files(paths: Vec<PathBuf>, output_dir: &Path) -> Result<ProcessResult, String> {
    // Validate first
    validate_files(&paths).map_err(|e| e.message)?;

    // Ensure output directory exists
    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    if paths.len() == 1 {
        let path = &paths[0];

        if is_image(path) && !is_webp(path) {
            // Single convertible image → WebP
            convert_to_webp(path, output_dir)
        } else {
            // Single non-image or already WebP → passthrough
            copy_file(path, output_dir)
        }
    } else {
        // Multiple files → ZIP
        create_zip(&paths, output_dir)
    }
}
