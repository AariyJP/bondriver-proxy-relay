pub mod bondriver;
pub mod sync;

use sync::Config;

#[tauri::command]
fn get_config() -> Config {
    bondriver::discover()
}

#[tauri::command]
async fn path_exists(path: String) -> bool {
    tauri::async_runtime::spawn_blocking(move || std::path::Path::new(path.trim()).exists()).await.unwrap_or(false)
}

#[tauri::command]
async fn apply_bondriver(cfg: Config) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || bondriver::apply_elevated(&cfg)).await.map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_config, path_exists, apply_bondriver])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
