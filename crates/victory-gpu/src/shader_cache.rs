//! Renderer integration for the reusable persistent shader library.
use std::path::PathBuf;
use std::sync::OnceLock;
use victory_shader::emit::HostShader;
use victory_shader_library::ShaderLibrary;

static CACHE: OnceLock<ShaderLibrary> = OnceLock::new();

/// Keeps translated shaders in `dir` from now on.
pub fn use_shader_cache(dir: PathBuf) {
    match ShaderLibrary::open(dir) {
        Ok(cache) => {
            let _ = CACHE.set(cache);
        }
        Err(e) => tracing::warn!(error = %e, "no shader cache"),
    }
}

pub fn counts() -> (u64, u64) {
    CACHE.get().map_or((0, 0), ShaderLibrary::counts)
}

/// What identifies this build: the BLAKE3 of our executable file and of
/// the translation settings it runs with (the loop limit, which a
/// diagnostics build can change at run time).
fn build() -> &'static [u8; 32] {
    static BUILD: OnceLock<[u8; 32]> = OnceLock::new();
    BUILD.get_or_init(|| {
        let exe = std::env::current_exe().and_then(std::fs::read);
        let limit = victory_shader::emit::loop_limit();
        if limit != victory_shader::emit::DEFAULT_LOOP_LIMIT {
            tracing::warn!(
                limit,
                "VICTORY_SHADER_LOOP_LIMIT: translated loops stop sooner"
            );
        }
        match exe {
            Ok(mut bytes) => {
                bytes.extend_from_slice(&limit.to_le_bytes());
                bytes.extend_from_slice(
                    &victory_shader::emit::configured_host_max_wave().to_le_bytes(),
                );
                bytes.extend_from_slice(&victory_shader::emit::view_heap_limit().to_le_bytes());
                // Which group barriers wait is part of the translation too.
                bytes.push(victory_shader::emit::group_sync());
                // So are the switches that pick the form of a wave64 program:
                // without them here a run with `VICTORY_WAVE_WIDE=0` was
                // served the translations an earlier run of the same
                // executable had cached with the switch unset.
                for name in [
                    "VICTORY_WAVE_WIDE",
                    "VICTORY_WAVE_DEEP_SKIPS",
                    "VICTORY_WAVE_OWN_BITS",
                    "VICTORY_F16_NEAREST",
                ] {
                    bytes.extend_from_slice(name.as_bytes());
                    bytes.push(b'=');
                    if let Some(value) = std::env::var_os(name) {
                        bytes.extend_from_slice(value.as_encoded_bytes());
                    }
                    bytes.push(0);
                }
                // And which clean-up passes ran over the translated code.
                bytes.extend_from_slice(&victory_shader::emit::shader_opt().to_le_bytes());
                *blake3::hash(&bytes).as_bytes()
            }
            Err(e) => {
                // Without it no entry can be trusted: a fresh key per run.
                tracing::warn!(error = %e, "cannot read our executable; shader cache unused");
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                *blake3::hash(&now.as_nanos().to_le_bytes()).as_bytes()
            }
        }
    })
}

/// Loads the translation from the library when configured.
pub(crate) fn host_shader(container: &[u8]) -> Result<HostShader, String> {
    let translate = |c: &[u8]| victory_shader::emit::translate(c).map_err(|e| e.to_string());
    match CACHE.get() {
        Some(cache) => cache.get_or_translate(build(), container, translate),
        None => translate(container),
    }
}

/// Initializes the same GPU-dependent translation settings used during play.
pub fn prepare_precompile(dir: PathBuf) -> Result<(), String> {
    crate::host::host().ok_or("cannot initialize the host GPU")?;
    crate::descriptor::host::probe_view_limit();
    let library = ShaderLibrary::open(dir).map_err(|e| e.to_string())?;
    CACHE
        .set(library)
        .map_err(|_| "shader library already initialized".to_owned())
}

/// Stores a translation using exactly the runtime's cache key and translator.
pub fn precompile(container: &[u8]) -> Result<(), String> {
    host_shader(container).map(|_| ())
}
