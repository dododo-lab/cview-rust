fn main() {
    println!("cargo:rerun-if-env-changed=CVIEW_UPDATE_URL");
    tauri_build::build()
}
