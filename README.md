# ZipDrop

A lightweight macOS menu bar app for instant file uploads to Cloudflare R2.

Drop files onto the menu bar icon, get a shareable URL in your clipboard. That's it.

## Features

- **Drag & Drop** - Drop files onto the menu bar icon to upload
- **Auto-Optimization** - Images are automatically converted to WebP for smaller file sizes
- **Multi-File Support** - Drop multiple files and they're zipped together automatically
- **Instant Clipboard** - URL is copied to your clipboard immediately after upload
- **Secure Storage** - R2 credentials are stored in macOS Keychain
- **Demo Mode** - Try it out locally before configuring cloud storage

## How It Works

1. Click the ZipDrop icon in your menu bar
2. Drag files into the drop zone
3. Files are processed (images → WebP, multiple files → ZIP)
4. Uploaded to your Cloudflare R2 bucket
5. Public URL copied to clipboard

## Requirements

- macOS 12.0 or later
- Cloudflare account with R2 storage (for cloud uploads)

## Setup

### Quick Start (Demo Mode)

1. Download and run ZipDrop
2. Demo mode is enabled by default - files save to `~/Downloads/ZipDrop`
3. Try dropping some files to see it in action

### Cloud Upload Setup

1. Open Settings (click gear icon)
2. Disable Demo Mode
3. Enter your Cloudflare R2 credentials:
   - **Account ID** - Found in Cloudflare dashboard
   - **Bucket Name** - Your R2 bucket name
   - **Access Key ID** - R2 API token access key
   - **Secret Access Key** - R2 API token secret key
   - **Public URL Base** - Your bucket's public URL or custom domain
4. Click "Test Credentials" to verify
5. Click "Save"

### Creating R2 API Credentials

1. Go to Cloudflare Dashboard → R2 → Manage R2 API Tokens
2. Create a new API token with "Object Read & Write" permissions
3. Copy the Access Key ID and Secret Access Key

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/)
- [pnpm](https://pnpm.io/)

### Running Locally

```bash
# Install dependencies
pnpm install

# Start development server
pnpm tauri dev
```

### Building for Production

```bash
pnpm tauri build
```

The built app will be in `src-tauri/target/release/bundle/`.

## Tech Stack

- **Framework**: [Tauri 2](https://tauri.app/) - Lightweight native app framework
- **Frontend**: React + TypeScript + Vite
- **Backend**: Rust
- **Storage**: Cloudflare R2 (S3-compatible)
- **Secrets**: macOS Keychain

## License

MIT
