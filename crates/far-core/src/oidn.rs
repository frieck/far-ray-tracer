//! Intel Open Image Denoise, the denoiser of Blender's Cycles and of V-Ray: a neural network
//! that removes path tracing noise while it keeps edges and texture.
//!
//! The library is loaded while the program runs, not linked: the app carries it, and without it
//! the engine falls back to its own filter in `denoise`. Its folder holds the libraries as the
//! OIDN release ships them, so they find one another.

use libloading::Library;
use std::ffi::{c_char, c_void, CStr};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

type Device = *mut c_void;
type Filter = *mut c_void;

const DEVICE_TYPE_CPU: i32 = 1;
const FORMAT_FLOAT3: i32 = 3;
const QUALITY_HIGH: i32 = 6;
const ERROR_NONE: i32 = 0;

/// The main library's file name on each system.
#[cfg(target_os = "macos")]
const LIBRARY: &str = "libOpenImageDenoise.2.dylib";
#[cfg(target_os = "windows")]
const LIBRARY: &str = "OpenImageDenoise.dll";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const LIBRARY: &str = "libOpenImageDenoise.so.2";

struct Api {
    new_device: unsafe extern "C" fn(i32) -> Device,
    commit_device: unsafe extern "C" fn(Device),
    device_error: unsafe extern "C" fn(Device, *mut *const c_char) -> i32,
    new_filter: unsafe extern "C" fn(Device, *const c_char) -> Filter,
    set_image: unsafe extern "C" fn(Filter, *const c_char, *mut c_void, i32, usize, usize, usize, usize, usize),
    set_bool: unsafe extern "C" fn(Filter, *const c_char, bool),
    set_int: unsafe extern "C" fn(Filter, *const c_char, i32),
    commit_filter: unsafe extern "C" fn(Filter),
    execute_filter: unsafe extern "C" fn(Filter),
    release_filter: unsafe extern "C" fn(Filter),
}

struct Oidn {
    api: Api,
    device: Device,
    // Unloading the library would leave `api` dangling, so it lives as long as the program.
    _library: Library,
}

// The device is only used while the mutex around it is held.
unsafe impl Send for Oidn {}

static OIDN: OnceLock<Option<Mutex<Oidn>>> = OnceLock::new();

/// Loads the library from `folder` and opens a CPU device, once for the whole program. Later
/// calls change nothing. Returns why it failed, and the engine keeps its own filter.
pub fn load(folder: &Path) -> Result<(), String> {
    let mut error = None;
    OIDN.get_or_init(|| match open(folder) {
        Ok(oidn) => Some(Mutex::new(oidn)),
        Err(e) => {
            error = Some(e);
            None
        }
    });
    match (error, available()) {
        (Some(e), _) => Err(e),
        (None, true) => Ok(()),
        (None, false) => Err("Open Image Denoise could not be loaded earlier".into()),
    }
}

/// Whether denoising uses Open Image Denoise.
pub fn available() -> bool {
    OIDN.get().is_some_and(|o| o.is_some())
}

fn open(folder: &Path) -> Result<Oidn, String> {
    let path = folder.join(LIBRARY);
    if !path.is_file() {
        return Err(format!("{} not found", path.display()));
    }
    // SAFETY: loading a library runs its initializers; this is Intel's release build.
    let library = unsafe { load_library(&path) }.map_err(|e| format!("{}: {e}", path.display()))?;
    // SAFETY: the signatures match the OIDN 2 C API in OpenImageDenoise/oidn.h.
    let api = unsafe {
        macro_rules! function {
            ($name:literal) => {
                *library.get($name).map_err(|e| format!("{}: {e}", String::from_utf8_lossy($name)))?
            };
        }
        Api {
            new_device: function!(b"oidnNewDevice\0"),
            commit_device: function!(b"oidnCommitDevice\0"),
            device_error: function!(b"oidnGetDeviceError\0"),
            new_filter: function!(b"oidnNewFilter\0"),
            set_image: function!(b"oidnSetSharedFilterImage\0"),
            set_bool: function!(b"oidnSetFilterBool\0"),
            set_int: function!(b"oidnSetFilterInt\0"),
            commit_filter: function!(b"oidnCommitFilter\0"),
            execute_filter: function!(b"oidnExecuteFilter\0"),
            release_filter: function!(b"oidnReleaseFilter\0"),
        }
    };
    // The CPU device works on the program's own memory, so images need no copies.
    let device = unsafe { (api.new_device)(DEVICE_TYPE_CPU) };
    if device.is_null() {
        return Err("Open Image Denoise has no CPU device".into());
    }
    unsafe { (api.commit_device)(device) };
    let oidn = Oidn { api, device, _library: library };
    oidn.check()?;
    Ok(oidn)
}

/// On Windows the libraries a DLL needs are looked up beside it only with this flag.
#[cfg(target_os = "windows")]
unsafe fn load_library(path: &Path) -> Result<Library, libloading::Error> {
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;
    unsafe { libloading::os::windows::Library::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH) }.map(Into::into)
}

#[cfg(not(target_os = "windows"))]
unsafe fn load_library(path: &Path) -> Result<Library, libloading::Error> {
    unsafe { Library::new(path) }
}

impl Oidn {
    fn check(&self) -> Result<(), String> {
        let mut message: *const c_char = std::ptr::null();
        let code = unsafe { (self.api.device_error)(self.device, &mut message) };
        if code == ERROR_NONE {
            return Ok(());
        }
        let text = if message.is_null() { String::new() } else { unsafe { CStr::from_ptr(message) }.to_string_lossy().into_owned() };
        Err(format!("Open Image Denoise error {code}: {text}"))
    }
}

/// Denoises a `width` × `height` image with its albedo and normal, which tell the network where
/// edges and textures are. `hdr` says `color` is linear radiance; otherwise it is in [0, 1].
/// None when the library is not loaded or the filter fails.
pub fn denoise(color: &[[f32; 3]], albedo: &[[f32; 3]], normal: &[[f32; 3]], width: usize, height: usize, hdr: bool)
    -> Option<Vec<[f32; 3]>> {
    let oidn = OIDN.get()?.as_ref()?.lock().ok()?;
    let api = &oidn.api;
    let pixels = width * height;
    if color.len() != pixels || albedo.len() != pixels || normal.len() != pixels || pixels == 0 {
        return None;
    }
    let mut output = vec![[0.0f32; 3]; pixels];
    let name = |text: &'static CStr| text.as_ptr();
    unsafe {
        let filter = (api.new_filter)(oidn.device, name(c"RT"));
        if filter.is_null() {
            return None;
        }
        let image = |label: &'static CStr, data: *const [f32; 3]| {
            (api.set_image)(filter, name(label), data as *mut c_void, FORMAT_FLOAT3, width, height, 0, 0, 0);
        };
        image(c"color", color.as_ptr());
        image(c"albedo", albedo.as_ptr());
        image(c"normal", normal.as_ptr());
        image(c"output", output.as_mut_ptr());
        (api.set_bool)(filter, name(c"hdr"), hdr);
        (api.set_int)(filter, name(c"quality"), QUALITY_HIGH);
        (api.commit_filter)(filter);
        (api.execute_filter)(filter);
        (api.release_filter)(filter);
    }
    oidn.check().ok()?;
    Some(output)
}
