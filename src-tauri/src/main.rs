#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if let [_, flag, engine_folder, model_file] = arguments.as_slice() {
        if flag == tarn_lib::SPEECH_WORKER_FLAG {
            std::process::exit(tarn_lib::run_speech_worker(engine_folder, model_file));
        }
    }
    tarn_lib::run()
}
