// FMap 桌面应用入口。
// release 构建隐藏控制台窗口（Windows），debug 构建保留以便看日志。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tracenest_desktop_lib::run()
}
