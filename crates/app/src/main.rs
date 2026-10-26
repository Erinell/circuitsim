//! Native and web entry points.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_maximized(true),
        ..Default::default()
    };
    eframe::run_native(
        circuitsim_app::APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(circuitsim_app::CircuitApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(canvas) = document
        .get_element_by_id("circuitsim-canvas")
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    else {
        return;
    };

    wasm_bindgen_futures::spawn_local(async move {
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(circuitsim_app::CircuitApp::new(cc)))),
            )
            .await;

        if let Some(loading) = document.get_element_by_id("circuitsim-loading") {
            if result.is_ok() {
                loading.remove();
            } else {
                loading
                    .set_inner_html("<p>CircuitSim n'a pas pu démarrer. Consultez la console.</p>");
            }
        }
    });
}
