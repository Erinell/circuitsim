//! Platform-specific CSV saving for simulation results.

use thiserror::Error;

/// Error raised while exporting generated text.
#[derive(Debug, Error)]
pub enum ExportError {
    /// No destination path was supplied.
    #[error("le chemin d'export est vide")]
    EmptyPath,
    /// The destination cannot be written.
    #[error("impossible d'écrire « {path} » : {message}")]
    Write {
        /// Destination path.
        path: String,
        /// Operating-system or browser error.
        message: String,
    },
    /// The browser download cannot be started.
    #[cfg(target_arch = "wasm32")]
    #[error("le téléchargement a échoué : {message}")]
    Browser {
        /// Browser error description.
        message: String,
    },
}

/// Saves UTF-8 text to a native file path.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_text(path: &str, contents: &str) -> Result<(), ExportError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(ExportError::EmptyPath);
    }
    std::fs::write(path, contents).map_err(|error| ExportError::Write {
        path: path.to_owned(),
        message: error.to_string(),
    })
}

/// Saves UTF-8 CSV to a native file path.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_csv(path: &str, contents: &str) -> Result<(), ExportError> {
    save_text(path, contents)
}

/// Starts a browser download for UTF-8 text.
#[cfg(target_arch = "wasm32")]
pub fn download_text(filename: &str, mime: &str, contents: &str) -> Result<(), ExportError> {
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::JsValue;

    let browser_error = |message: String| ExportError::Browser { message };
    let window =
        web_sys::window().ok_or_else(|| browser_error("fenêtre indisponible".to_owned()))?;
    let document = window
        .document()
        .ok_or_else(|| browser_error("document indisponible".to_owned()))?;
    let parts = js_sys::Array::of1(&JsValue::from_str(contents));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let blob = web_sys::Blob::new_with_str_sequence_and_options(parts.as_ref(), &options)
        .map_err(|error| browser_error(format!("{error:?}")))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|error| browser_error(format!("{error:?}")))?;
    let element = document
        .create_element("a")
        .map_err(|error| browser_error(format!("{error:?}")))?;
    let anchor = element
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| browser_error("lien de téléchargement invalide".to_owned()))?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    let body = document
        .body()
        .ok_or_else(|| browser_error("corps du document indisponible".to_owned()))?;
    body.append_child(&anchor)
        .map_err(|error| browser_error(format!("{error:?}")))?;
    anchor.click();
    anchor.remove();
    web_sys::Url::revoke_object_url(&url).map_err(|error| browser_error(format!("{error:?}")))?;
    Ok(())
}

/// Starts a browser download for UTF-8 CSV content.
#[cfg(target_arch = "wasm32")]
pub fn download_csv(filename: &str, contents: &str) -> Result<(), ExportError> {
    download_text(filename, "text/csv;charset=utf-8", contents)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn native_export_rejects_an_empty_path() {
        assert!(matches!(
            save_csv("   ", "signal\n"),
            Err(ExportError::EmptyPath)
        ));
    }

    #[test]
    fn native_export_round_trips_utf8() {
        let path =
            std::env::temp_dir().join(format!("circuitsim-export-{}.csv", std::process::id()));
        let path_text = path.to_string_lossy().into_owned();
        save_csv(&path_text, "signal,unit\nV(OUT),V\n").unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "signal,unit\nV(OUT),V\n");
        std::fs::remove_file(&path).unwrap();
    }
}
