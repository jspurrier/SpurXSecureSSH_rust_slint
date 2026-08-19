// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Work around WebKitGTK DMA-BUF / EGL renderer crash with NVIDIA proprietary drivers while preserving GPU acceleration
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if std::env::var_os("WEBKIT_FORCE_COMPOSITING_MODE").is_none() {
            std::env::set_var("WEBKIT_FORCE_COMPOSITING_MODE", "1");
        }
        if std::env::var_os("WEBKIT_USE_GL").is_none() {
            std::env::set_var("WEBKIT_USE_GL", "gles");
        }
    }

    spurx_secure_ssh_lib::run()
}
