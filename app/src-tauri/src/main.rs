// Windowsのリリースビルドで、余分なコンソールの窓を出さない（Tauriの手順どおり）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    scn_app_lib::run()
}
