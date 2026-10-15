pub mod bondriver;
pub mod sync;

use sync::Config;

#[tauri::command]
fn get_config() -> Result<Config, String> {
    sync::load()
}

#[tauri::command]
fn save_config(cfg: Config) -> Result<(), String> {
    sync::save(&cfg)
}

#[tauri::command]
fn config_path() -> String {
    sync::config_path().display().to_string()
}

#[tauri::command]
async fn list_own(cfg: Config) -> Result<Vec<String>, String> {
    let rows = tauri::async_runtime::spawn_blocking(move || sync::list_own(&cfg)).await.map_err(|e| e.to_string())??;
    Ok(rows.into_iter().map(|(n, _, _)| n).collect())
}

#[tauri::command]
async fn run_pull() -> Result<Vec<String>, String> {
    let cfg = sync::load()?;
    tauri::async_runtime::spawn_blocking(move || sync::pull(&cfg)).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn apply_bondriver() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(bondriver::apply_elevated).await.map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_config, save_config, config_path, list_own, run_pull, apply_bondriver])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
