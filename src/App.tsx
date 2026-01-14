import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

// =============================================================================
// CONSTANTS
// =============================================================================
const MAX_FILES = 50;
const DEBOUNCE_MS = 300;
const MAX_RECENT_UPLOADS = 10;

const ALLOWED_EXTENSIONS = new Set([
  // Images
  "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "webp", "heic", "heif", "svg", "ico", "raw", "cr2", "nef", "arw",
  // Documents
  "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "md", "markdown", "pages", "numbers", "key",
  // Archives
  "zip", "tar", "gz", "7z", "rar", "bz2", "xz", "tgz",
  // Video
  "mov", "mp4", "avi", "mkv", "webm", "m4v", "wmv", "flv", "3gp",
  // Audio
  "mp3", "wav", "aac", "flac", "m4a", "ogg", "wma", "aiff",
  // Code & Data
  "json", "xml", "html", "css", "js", "ts", "jsx", "tsx", "py", "rs", "go", "swift", "java", "c", "cpp", "h", "rb", "php", "sh", "bash", "zsh", "yaml", "yml", "toml", "ini", "sql", "graphql",
  // macOS/Apps
  "dmg", "pkg", "app", "ipa",
  // Fonts
  "ttf", "otf", "woff", "woff2", "eot",
  // Other
  "log", "env", "gitignore", "dockerfile",
]);

// =============================================================================
// TYPES
// =============================================================================
type AppState = "idle" | "drag-over" | "processing" | "success" | "error";

interface UploadItem {
  id: string;
  name: string;
  size: string;
  url: string;
  localPath?: string;
  r2Key?: string;
  isDemo: boolean;
  timestamp: number;
}

interface DropResult {
  url: string;
  local_path: string | null;
  r2_key: string | null;
  original_size: number;
  processed_size: number;
  file_type: string;
  is_demo: boolean;
}

interface ConfigStatus {
  is_configured: boolean;
  demo_mode: boolean;
  bucket_name: string | null;
}

interface R2Config {
  access_key: string;
  secret_key: string;
  bucket_name: string;
  account_id: string;
  public_url_base: string;
}

interface TauriDropEvent {
  paths: string[];
  position: { x: number; y: number };
}

// =============================================================================
// UTILITY FUNCTIONS
// =============================================================================
function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
}

function getExtension(path: string): string {
  const parts = path.split(".");
  return parts.length > 1 ? parts.pop()!.toLowerCase() : "";
}

function getFileName(path: string): string {
  return path.split("/").pop() || path;
}

function validateFiles(paths: string[]): { valid: boolean; error?: string } {
  if (paths.length === 0) {
    return { valid: false, error: "No files provided" };
  }
  if (paths.length > MAX_FILES) {
    return { valid: false, error: `Too many files. Maximum is ${MAX_FILES}.` };
  }
  for (const path of paths) {
    const ext = getExtension(path);
    if (ext && !ALLOWED_EXTENSIONS.has(ext)) {
      const fileName = getFileName(path);
      return { valid: false, error: `Unsupported file type: .${ext} (${fileName})` };
    }
  }
  return { valid: true };
}

// =============================================================================
// MAIN APP COMPONENT
// =============================================================================
function App() {
  const [state, setState] = useState<AppState>("idle");
  const [statusText, setStatusText] = useState("");
  const [uploads, setUploads] = useState<UploadItem[]>([]);
  const [configStatus, setConfigStatus] = useState<ConfigStatus>({
    is_configured: false,
    demo_mode: true,
    bucket_name: null,
  });
  const [showSettings, setShowSettings] = useState(false);
  const [currentFileName, setCurrentFileName] = useState("");

  // Refs
  const lastDropTime = useRef(0);
  const isProcessing = useRef(false);

  // Load config and uploads on mount
  useEffect(() => {
    invoke<ConfigStatus>("get_config_status").then(setConfigStatus);
    const saved = localStorage.getItem("zipdrop_uploads");
    if (saved) {
      try {
        setUploads(JSON.parse(saved));
      } catch {
        // Invalid JSON
      }
    }

    // Ensure window can receive keyboard events
    window.focus();
  }, []);

  // Save uploads to localStorage whenever they change
  useEffect(() => {
    localStorage.setItem("zipdrop_uploads", JSON.stringify(uploads));
  }, [uploads]);

  // Set up event listeners
  useEffect(() => {
    // Process files function
    const processFiles = async (paths: string[]) => {
      // Debounce
      const now = Date.now();
      if (now - lastDropTime.current < DEBOUNCE_MS) {
        console.log("Debounced");
        return;
      }
      lastDropTime.current = now;

      // Don't process if already processing
      if (isProcessing.current) {
        console.log("Already processing");
        return;
      }

      if (paths.length === 0) return;

      // Validate
      const validation = validateFiles(paths);
      if (!validation.valid) {
        setState("error");
        setStatusText(validation.error!);
        setTimeout(() => {
          setState("idle");
          setStatusText("");
        }, 3000);
        return;
      }

      // Start processing
      isProcessing.current = true;
      const fileNames = paths.map(getFileName);
      setCurrentFileName(fileNames[0]);
      setState("processing");
      setStatusText(`Processing ${paths.length} file${paths.length > 1 ? "s" : ""}...`);

      try {
        console.log("Invoking process_and_upload with:", paths);
        const result = await invoke<DropResult>("process_and_upload", { paths });
        console.log("Result:", result);

        // Build display name
        let displayName: string;
        if (paths.length > 1) {
          displayName = "archive.zip";
        } else {
          const baseName = fileNames[0].replace(/\.[^.]+$/, "");
          displayName = `${baseName}.${result.file_type}`;
        }

        // Create upload item
        const newUpload: UploadItem = {
          id: Date.now().toString(),
          name: displayName,
          size: formatBytes(result.processed_size),
          url: result.url,
          localPath: result.local_path || undefined,
          r2Key: result.r2_key || undefined,
          isDemo: result.is_demo,
          timestamp: Date.now(),
        };

        console.log("Adding upload:", newUpload);

        // Update uploads state
        setUploads(prev => {
          const updated = [newUpload, ...prev].slice(0, MAX_RECENT_UPLOADS);
          console.log("Updated uploads:", updated);
          return updated;
        });

        setState("success");
        setStatusText(result.is_demo ? "Saved to Downloads!" : "Copied to clipboard!");

        // Reset UI after 2 seconds, then auto-hide window after 10 seconds
        setTimeout(() => {
          setState("idle");
          setStatusText("");
          setCurrentFileName("");
        }, 2000);

        setTimeout(() => {
          console.log("Auto-hiding window after 10 seconds");
          getCurrentWindow().hide();
        }, 10000);

      } catch (error) {
        console.error("Error:", error);
        setState("error");
        setStatusText(String(error));
        setTimeout(() => {
          setState("idle");
          setStatusText("");
          setCurrentFileName("");
        }, 3000);
      } finally {
        isProcessing.current = false;
      }
    };

    // Keyboard handler
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        getCurrentWindow().hide();
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    // Tauri event listeners
    console.log("Setting up Tauri drag-drop listeners...");
    
    const dropListener = listen<TauriDropEvent>("tauri://drag-drop", (event) => {
      console.log("DROP EVENT RECEIVED:", event.payload);
      if (event.payload.paths?.length > 0) {
        processFiles(event.payload.paths);
      }
    });

    const dragOverListener = listen("tauri://drag-over", (event) => {
      console.log("DRAG OVER EVENT:", event);
      if (!isProcessing.current) {
        setState("drag-over");
      }
    });

    const dragLeaveListener = listen("tauri://drag-leave", (event) => {
      console.log("DRAG LEAVE EVENT:", event);
      if (!isProcessing.current) {
        setState("idle");
      }
    });
    
    dropListener.then(() => console.log("Drop listener registered"));
    dragOverListener.then(() => console.log("DragOver listener registered"));
    dragLeaveListener.then(() => console.log("DragLeave listener registered"));

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      dropListener.then(fn => fn());
      dragOverListener.then(fn => fn());
      dragLeaveListener.then(fn => fn());
    };
  }, []); // Empty deps - only run once

  const copyUrl = async (url: string) => {
    try {
      await invoke("copy_to_clipboard", { text: url });
    } catch {
      navigator.clipboard.writeText(url);
    }
  };

  const openUrl = (url: string, localPath?: string) => {
    if (localPath) {
      invoke("reveal_in_finder", { path: localPath });
    } else {
      invoke("open_in_browser", { url });
    }
  };

  const deleteUpload = (e: React.MouseEvent, item: UploadItem) => {
    // Remove from UI immediately
    setUploads(prev => prev.filter(u => u.id !== item.id));
    
    // If shift+click on R2 upload, fire-and-forget delete from cloud
    if (e.shiftKey && item.r2Key && !item.isDemo) {
      invoke("delete_from_r2", { key: item.r2Key }); // no await - async delete
    }
  };

  const handleConfigSaved = () => {
    invoke<ConfigStatus>("get_config_status").then(setConfigStatus);
    setShowSettings(false);
  };

  const toggleDemoMode = async () => {
    const newMode = !configStatus.demo_mode;
    await invoke("set_demo_mode", { enabled: newMode });
    setConfigStatus(prev => ({ ...prev, demo_mode: newMode }));
  };

  // Drag event handlers (for visual feedback)
  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <div className="container">
      {showSettings ? (
        <SettingsPanel
          onSave={handleConfigSaved}
          onClose={() => setShowSettings(false)}
          configStatus={configStatus}
          onToggleDemo={toggleDemoMode}
        />
      ) : (
        <>
          <div className="header">
            <span className="title">ZipDrop</span>
            <div className="header-actions">
              {configStatus.demo_mode && <span className="demo-badge">Demo</span>}
              <button
                className="settings-btn"
                title="Settings"
                onClick={() => setShowSettings(true)}
              >
                <SettingsIcon />
              </button>
            </div>
          </div>

          <div
            className={`drop-zone ${state}`}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
          >
            {state === "processing" ? (
              <>
                <div className="drop-icon spinning"><SpinnerIcon /></div>
                <div className="drop-text">Processing...</div>
                <div className="drop-subtext">{currentFileName || "\u00A0"}</div>
              </>
            ) : state === "success" ? (
              <>
                <div className="drop-icon success"><CheckIcon /></div>
                <div className="drop-text">{statusText}</div>
                <div className="drop-subtext">&nbsp;</div>
              </>
            ) : state === "error" ? (
              <>
                <div className="drop-icon error"><ErrorIcon /></div>
                <div className="drop-text error">{statusText || "Error"}</div>
                <div className="drop-subtext">&nbsp;</div>
              </>
            ) : state === "drag-over" ? (
              <>
                <div className="drop-icon"><UploadIcon /></div>
                <div className="drop-text">Drop to upload</div>
                <div className="drop-subtext">&nbsp;</div>
              </>
            ) : (
              <>
                <div className="drop-icon"><UploadIcon /></div>
                <div className="drop-text">Drop files here</div>
                <div className="drop-subtext">
                  Images → WebP • Multiple → ZIP
                  {configStatus.demo_mode && " • Saves locally"}
                </div>
              </>
            )}
          </div>

          <div className={`progress-container ${state === "processing" ? "visible" : ""}`}>
            <div className="progress-bar">
              <div className="progress-fill indeterminate" />
            </div>
            <div className="status-text">{state === "processing" ? statusText : "\u00A0"}</div>
          </div>

          <div className="recent-section">
            <div className="section-header">
              <span className="section-title">Recent</span>
              {uploads.length > 0 && (
                <button
                  className="clear-btn"
                  onClick={() => setUploads([])}
                  title="Clear all"
                >
                  Clear
                </button>
              )}
            </div>

            <div className="upload-list">
              {uploads.length === 0 ? (
                <div className="empty-state">No uploads yet</div>
              ) : (
                uploads.map((item) => (
                  <div
                    key={item.id}
                    className="upload-item"
                    onClick={() => copyUrl(item.localPath || item.url)}
                  >
                    <div className="item-thumb file-icon"><FileIcon /></div>
                    <div className="item-info">
                      <div className="item-name">{item.name}</div>
                      <div className="item-meta">
                        <span className="item-size">{item.size}</span>
                        {item.isDemo && <span className="item-badge">Local</span>}
                      </div>
                    </div>
                    <div className="item-actions">
                      <button
                        className="action-btn"
                        title="Copy"
                        onClick={(e) => { e.stopPropagation(); copyUrl(item.localPath || item.url); }}
                      >
                        <LinkIcon />
                      </button>
                      <button
                        className="action-btn"
                        title={item.isDemo ? "Show in Finder" : "Open"}
                        onClick={(e) => { e.stopPropagation(); openUrl(item.url, item.localPath); }}
                      >
                        {item.isDemo ? <FolderIcon /> : <ExternalIcon />}
                      </button>
                      <button
                        className="action-btn delete"
                        title={item.isDemo ? "Remove" : "Remove (Shift+click to delete from cloud)"}
                        onClick={(e) => { e.stopPropagation(); deleteUpload(e, item); }}
                      >
                        <TrashIcon />
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

// =============================================================================
// SETTINGS PANEL
// =============================================================================
function SettingsPanel({
  onSave,
  onClose,
  configStatus,
  onToggleDemo,
}: {
  onSave: () => void;
  onClose: () => void;
  configStatus: ConfigStatus;
  onToggleDemo: () => void;
}) {
  const [accessKey, setAccessKey] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [bucketName, setBucketName] = useState("");
  const [accountId, setAccountId] = useState("");
  const [publicUrlBase, setPublicUrlBase] = useState("");
  const [saving, setSaving] = useState(false);
  const [validating, setValidating] = useState(false);
  const [error, setError] = useState("");
  const [validated, setValidated] = useState(false);

  // Load existing config on mount
  useEffect(() => {
    invoke<R2Config | null>("get_r2_config").then((config) => {
      if (config) {
        setAccessKey(config.access_key);
        setSecretKey(config.secret_key);
        setBucketName(config.bucket_name);
        setAccountId(config.account_id);
        setPublicUrlBase(config.public_url_base);
        setValidated(true); // Existing config is assumed valid
      }
    }).catch(() => {
      // No config saved yet
    });
  }, []);

  // Reset validated state when credentials change
  useEffect(() => {
    setValidated(false);
    setError("");
  }, [accessKey, secretKey, bucketName, accountId]);

  const isDisabled = configStatus.demo_mode;

  const handleValidate = async () => {
    if (!accessKey || !secretKey || !bucketName || !accountId) {
      setError("Account ID, Bucket Name, Access Key, and Secret Key are required");
      return;
    }

    setValidating(true);
    setError("");

    try {
      await invoke("validate_r2_config", {
        config: { access_key: accessKey, secret_key: secretKey, bucket_name: bucketName, account_id: accountId, public_url_base: publicUrlBase || "https://example.com" }
      });
      setValidated(true);
    } catch (e) {
      setError(String(e));
      setValidated(false);
    } finally {
      setValidating(false);
    }
  };

  const handleSave = async () => {
    if (!accessKey || !secretKey || !bucketName || !accountId || !publicUrlBase) {
      setError("All fields are required");
      return;
    }

    if (!validated) {
      setError("Please test your credentials first");
      return;
    }

    setSaving(true);
    setError("");

    try {
      await invoke("set_r2_config", {
        config: { access_key: accessKey, secret_key: secretKey, bucket_name: bucketName, account_id: accountId, public_url_base: publicUrlBase }
      });
      onSave();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="settings-panel">
      <div className="settings-header">
        <span className="title">Settings</span>
        <button className="close-btn" onClick={onClose}>×</button>
      </div>

      <div className="settings-form">
        <div className="setting-row">
          <div className="setting-info">
            <span className="setting-label">Demo Mode</span>
            <span className="setting-desc">Save files locally without uploading</span>
          </div>
          <button className={`toggle-btn ${configStatus.demo_mode ? "active" : ""}`} onClick={onToggleDemo}>
            <div className="toggle-knob" />
          </button>
        </div>

        <div className="settings-divider" />
        <div className="settings-section-title">Cloudflare R2</div>
        {isDisabled && <span className="form-hint">Disable Demo Mode to edit R2 settings</span>}

        <div className="form-group">
          <label>Account ID</label>
          <input type="text" value={accountId} onChange={(e) => setAccountId(e.target.value)} placeholder="Your Cloudflare account ID" disabled={isDisabled} />
        </div>

        <div className="form-group">
          <label>Bucket Name</label>
          <input type="text" value={bucketName} onChange={(e) => setBucketName(e.target.value)} placeholder="my-bucket" disabled={isDisabled} />
        </div>

        <div className="form-group">
          <label>Access Key ID</label>
          <input type="text" value={accessKey} onChange={(e) => setAccessKey(e.target.value)} placeholder="R2 access key" disabled={isDisabled} />
        </div>

        <div className="form-group">
          <label>Secret Access Key</label>
          <input type="password" value={secretKey} onChange={(e) => setSecretKey(e.target.value)} placeholder="R2 secret key" disabled={isDisabled} />
        </div>

        <div className="form-group">
          <label>Public URL Base</label>
          <input type="text" value={publicUrlBase} onChange={(e) => setPublicUrlBase(e.target.value)} placeholder="https://zipdrop.co" disabled={isDisabled} />
          <span className="form-hint">Your R2 public bucket URL or custom domain</span>
        </div>

        {error && <div className="form-error">{error}</div>}
        {validated && !error && <div className="form-success">Credentials verified for: <strong>{bucketName}</strong></div>}

        <div className="button-row">
          <button className="test-btn" onClick={handleValidate} disabled={validating || isDisabled}>
            {validating ? "Testing..." : validated ? "Re-test" : "Test Credentials"}
          </button>
          <button className="save-btn" onClick={handleSave} disabled={saving || isDisabled || !validated}>
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}

// =============================================================================
// ICONS
// =============================================================================
const UploadIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 16V4M12 4l-4 4M12 4l4 4" /><path d="M3 17v3a2 2 0 002 2h14a2 2 0 002-2v-3" />
  </svg>
);

const SettingsIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z" />
  </svg>
);

const FileIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" /><path d="M14 2v6h6M16 13H8M16 17H8M10 9H8" />
  </svg>
);

const LinkIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71" /><path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71" />
  </svg>
);

const ExternalIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6M15 3h6v6M10 14L21 3" />
  </svg>
);

const FolderIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
  </svg>
);

const TrashIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 6h18M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2" />
  </svg>
);

const SpinnerIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83" />
  </svg>
);

const CheckIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M20 6L9 17l-5-5" />
  </svg>
);

const ErrorIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="10" /><path d="M15 9l-6 6M9 9l6 6" />
  </svg>
);

export default App;
