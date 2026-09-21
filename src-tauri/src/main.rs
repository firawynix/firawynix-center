// Sem console atrás da janela no build de release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    firawynix_center_lib::run()
}
