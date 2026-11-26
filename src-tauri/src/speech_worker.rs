use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use libloading::Library;
use serde::{Deserialize, Serialize};

pub const WORKER_FLAG: &str = "--speech-worker";

const REQUEST_HEADER_BYTES: usize = 13;
const BYTES_PER_SAMPLE: usize = 2;
const MIN_SAMPLES_TO_RECOGNIZE: usize = 1600;
const MAX_RECOGNITION_THREADS: usize = 4;
const GREEDY_STRATEGY: c_int = 0;
const SAMPLE_SCALE: f32 = 32768.0;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct WorkerReply {
    pub id: u64,
    pub text: Option<String>,
    pub error: Option<String>,
}

pub fn encode_request(id: u64, is_live: bool, pcm: &[u8]) -> Vec<u8> {
    let mut request = Vec::with_capacity(REQUEST_HEADER_BYTES + pcm.len());
    request.extend_from_slice(&id.to_le_bytes());
    request.push(u8::from(is_live));
    request.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    request.extend_from_slice(pcm);
    request
}

#[derive(Debug, PartialEq)]
struct RequestHeader {
    id: u64,
    is_live: bool,
    audio_bytes: usize,
}

fn parse_request_header(header: &[u8; REQUEST_HEADER_BYTES]) -> RequestHeader {
    let mut id = [0u8; 8];
    id.copy_from_slice(&header[..8]);
    let mut audio_bytes = [0u8; 4];
    audio_bytes.copy_from_slice(&header[9..13]);
    RequestHeader {
        id: u64::from_le_bytes(id),
        is_live: header[8] != 0,
        audio_bytes: u32::from_le_bytes(audio_bytes) as usize,
    }
}

fn samples_from_pcm(pcm: &[u8]) -> Vec<f32> {
    pcm.chunks_exact(BYTES_PER_SAMPLE)
        .map(|bytes| f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / SAMPLE_SCALE)
        .collect()
}

fn joined_text<'a>(pieces: impl Iterator<Item = &'a str>) -> String {
    pieces.map(str::trim).filter(|piece| !piece.is_empty()).collect::<Vec<_>>().join(" ")
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ContextParams {
    use_gpu: bool,
    gpu_device: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FullParams {
    strategy: c_int,
    n_threads: c_int,
    offset_ms: c_int,
    duration_ms: c_int,
    no_context: bool,
    audio_ctx: c_int,
    new_segment_callback: *mut c_void,
    new_segment_callback_user_data: *mut c_void,
    new_token_callback: *mut c_void,
    new_token_callback_user_data: *mut c_void,
    progress_callback: *mut c_void,
    progress_callback_user_data: *mut c_void,
    encoder_begin_callback: *mut c_void,
    encoder_begin_callback_user_data: *mut c_void,
    abort_callback: *mut c_void,
    abort_callback_user_data: *mut c_void,
}

type BackendLoadAll = unsafe extern "C" fn();
type ContextDefaultParams = unsafe extern "C" fn() -> ContextParams;
type InitFromFile = unsafe extern "C" fn(*const c_char, ContextParams) -> *mut c_void;
type InitState = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type FullDefaultParams = unsafe extern "C" fn(c_int) -> FullParams;
type FullWithState = unsafe extern "C" fn(*mut c_void, *mut c_void, FullParams, *const f32, c_int) -> c_int;
type SegmentCount = unsafe extern "C" fn(*mut c_void) -> c_int;
type SegmentText = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_char;

#[derive(Clone, Copy)]
struct ParakeetApi {
    init_state: InitState,
    full_default_params: FullDefaultParams,
    full_with_state: FullWithState,
    segment_count: SegmentCount,
    segment_text: SegmentText,
}

#[derive(Clone, Copy)]
struct SharedContext(*mut c_void);

unsafe impl Send for SharedContext {}

impl SharedContext {
    fn pointer(self) -> *mut c_void {
        self.0
    }
}

struct RecognitionJob {
    id: u64,
    samples: Vec<f32>,
}

#[cfg(windows)]
fn load_library(path: &Path) -> Result<Library, String> {
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;
    unsafe { libloading::os::windows::Library::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH) }
        .map(Library::from)
        .map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn load_library(path: &Path) -> Result<Library, String> {
    unsafe { Library::new(path) }.map_err(|error| error.to_string())
}

unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
    library.get::<T>(name).map(|found| *found).map_err(|error| error.to_string())
}

fn load_model(engine_folder: &Path, model_file: &Path) -> Result<(ParakeetApi, SharedContext), String> {
    let model_folder = model_file.parent().ok_or("нет папки модели")?;
    let model_name = model_file.file_name().and_then(|name| name.to_str()).ok_or("нет имени модели")?;
    let model_name = CString::new(model_name).map_err(|error| error.to_string())?;
    let ggml = load_library(&engine_folder.join("ggml.dll"))?;
    let parakeet = load_library(&engine_folder.join("parakeet.dll"))?;
    unsafe {
        let backend_load_all: BackendLoadAll = symbol(&ggml, b"ggml_backend_load_all\0")?;
        let context_default_params: ContextDefaultParams = symbol(&parakeet, b"parakeet_context_default_params\0")?;
        let init_from_file: InitFromFile = symbol(&parakeet, b"parakeet_init_from_file_with_params\0")?;
        let api = ParakeetApi {
            init_state: symbol(&parakeet, b"parakeet_init_state\0")?,
            full_default_params: symbol(&parakeet, b"parakeet_full_default_params\0")?,
            full_with_state: symbol(&parakeet, b"parakeet_full_with_state\0")?,
            segment_count: symbol(&parakeet, b"parakeet_full_n_segments_from_state\0")?,
            segment_text: symbol(&parakeet, b"parakeet_full_get_segment_text_from_state\0")?,
        };
        std::env::set_current_dir(engine_folder).map_err(|error| error.to_string())?;
        backend_load_all();
        std::env::set_current_dir(model_folder).map_err(|error| error.to_string())?;
        let context = init_from_file(model_name.as_ptr(), context_default_params());
        if context.is_null() {
            return Err("модель не загрузилась".to_string());
        }
        std::mem::forget(ggml);
        std::mem::forget(parakeet);
        Ok((api, SharedContext(context)))
    }
}

fn recognize(api: ParakeetApi, context: *mut c_void, state: *mut c_void, threads: c_int, samples: &[f32]) -> Result<String, String> {
    if samples.len() < MIN_SAMPLES_TO_RECOGNIZE {
        return Ok(String::new());
    }
    unsafe {
        let mut params = (api.full_default_params)(GREEDY_STRATEGY);
        params.n_threads = threads;
        let code = (api.full_with_state)(context, state, params, samples.as_ptr(), samples.len() as c_int);
        if code != 0 {
            return Err(format!("код ошибки {code}"));
        }
        let pieces: Vec<String> = (0..(api.segment_count)(state))
            .map(|index| CStr::from_ptr((api.segment_text)(state, index)).to_string_lossy().into_owned())
            .collect();
        Ok(joined_text(pieces.iter().map(String::as_str)))
    }
}

fn send_reply(output: &Mutex<std::io::Stdout>, reply: &WorkerReply) {
    let Ok(line) = serde_json::to_string(reply) else {
        return;
    };
    let Ok(mut stream) = output.lock() else {
        return;
    };
    let _ = writeln!(stream, "{line}");
    let _ = stream.flush();
}

fn start_recognizer(api: ParakeetApi, context: SharedContext, threads: c_int, output: Arc<Mutex<std::io::Stdout>>) -> Sender<RecognitionJob> {
    let (sender, receiver) = mpsc::channel::<RecognitionJob>();
    std::thread::spawn(move || {
        let context_pointer = context.pointer();
        let state = unsafe { (api.init_state)(context_pointer) };
        for job in receiver {
            let outcome = if state.is_null() {
                Err("не хватило памяти".to_string())
            } else {
                recognize(api, context_pointer, state, threads, &job.samples)
            };
            let (text, error) = match outcome {
                Ok(text) => (Some(text), None),
                Err(error) => (None, Some(error)),
            };
            send_reply(&output, &WorkerReply { id: job.id, text, error });
        }
    });
    sender
}

fn recognition_threads() -> c_int {
    let cores = std::thread::available_parallelism().map_or(2, |count| count.get());
    (cores / 2).clamp(1, MAX_RECOGNITION_THREADS) as c_int
}

fn serve_requests(live: &Sender<RecognitionJob>, refining: &Sender<RecognitionJob>) {
    let mut input = std::io::stdin().lock();
    let mut header = [0u8; REQUEST_HEADER_BYTES];
    while input.read_exact(&mut header).is_ok() {
        let request = parse_request_header(&header);
        let mut pcm = vec![0u8; request.audio_bytes];
        if input.read_exact(&mut pcm).is_err() {
            return;
        }
        let job = RecognitionJob { id: request.id, samples: samples_from_pcm(&pcm) };
        let target = if request.is_live { live } else { refining };
        if target.send(job).is_err() {
            return;
        }
    }
}

pub fn run(engine_folder: &str, model_file: &str) -> i32 {
    let (api, context) = match load_model(Path::new(engine_folder), Path::new(model_file)) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let output = Arc::new(Mutex::new(std::io::stdout()));
    let threads = recognition_threads();
    let live = start_recognizer(api, context, threads, output.clone());
    let refining = start_recognizer(api, context, threads, output);
    serve_requests(&live, &refining);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_header_survives_encoding() {
        let request = encode_request(7, true, &[1, 0, 2, 0]);
        let mut header = [0u8; REQUEST_HEADER_BYTES];
        header.copy_from_slice(&request[..REQUEST_HEADER_BYTES]);
        assert_eq!(parse_request_header(&header), RequestHeader { id: 7, is_live: true, audio_bytes: 4 });
        assert_eq!(&request[REQUEST_HEADER_BYTES..], &[1, 0, 2, 0]);
    }

    #[test]
    fn audio_bytes_become_samples_between_minus_one_and_one() {
        let samples = samples_from_pcm(&[0x00, 0x80, 0x00, 0x00, 0x00, 0x40]);
        assert_eq!(samples, vec![-1.0, 0.0, 0.5]);
    }

    #[test]
    fn recognized_pieces_join_without_empty_ones() {
        assert_eq!(joined_text([" Привет ", "", "мир\n"].into_iter()), "Привет мир");
        assert_eq!(joined_text(["  "].into_iter()), "");
    }

    #[test]
    fn recognition_never_takes_more_than_four_threads() {
        assert!((1..=MAX_RECOGNITION_THREADS as c_int).contains(&recognition_threads()));
    }
}
