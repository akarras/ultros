//! Planning stays client-only like its market resource. SSR and first hydrate
//! keep the existing skeleton; every browser calculation runs in a worker.

use leptos::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use ultros_calc::recipe_work::compute;
use ultros_calc::recipe_work::{Output, Request};

thread_local! {
    static PLANNER: std::cell::RefCell<Option<ultros_calc::recipe_work::Planner>> = const { std::cell::RefCell::new(None) };
}

pub fn compute_encoded(request: &str) -> Result<String, String> {
    let request = serde_json::from_str(request).map_err(|e| e.to_string())?;
    PLANNER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let planner = slot.get_or_insert_with(Default::default);
        serde_json::to_string(&planner.compute(request)).map_err(|e| e.to_string())
    })
}

pub(crate) fn use_planning(
    request: Memo<Option<Request>>,
) -> LocalResource<Result<Option<Output>, String>> {
    #[cfg(target_arch = "wasm32")]
    let planner = {
        let planner = browser::create(&wasm_bindgen::module());
        on_cleanup(move || browser::dispose(planner));
        planner
    };
    LocalResource::new(move || {
        let request = request.get();
        async move {
            let Some(request) = request else {
                return Ok(None);
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                Ok(Some(compute(request)))
            }
            #[cfg(target_arch = "wasm32")]
            {
                // Only serialization and result decoding run on this thread.
                // Pinned routes and output comparisons use the worker too.
                async move {
                    let payload = serde_json::to_string(&request).map_err(|e| e.to_string())?;
                    let value =
                        wasm_bindgen_futures::JsFuture::from(browser::plan(planner, &payload))
                            .await
                            .map_err(|e| format!("{e:?}"))?;
                    let text = value.as_string().ok_or("Invalid recipe worker response")?;
                    serde_json::from_str(&text)
                        .map(Some)
                        .map_err(|e| e.to_string())
                }
                .await
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(module = "/../../ultros/static/recipe-worker-client.mjs")]
    extern "C" {
        #[wasm_bindgen(js_name = createRecipePlanner)]
        pub fn create(module: &JsValue) -> u32;
        #[wasm_bindgen(js_name = planRecipe)]
        pub fn plan(id: u32, payload: &str) -> js_sys::Promise;
        #[wasm_bindgen(js_name = disposeRecipePlanner)]
        pub fn dispose(id: u32);
    }
}
