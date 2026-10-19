use crate::insert::Options;
use crate::inspect::inspect_project;
use crate::{Insertion, Project, Settings, Source, insert_reference};
use wasm_bindgen::prelude::*;

fn js_error(message: String) -> JsError {
    JsError::new(&message)
}

#[wasm_bindgen]
impl Settings {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Settings {
        Settings::default()
    }
}

#[wasm_bindgen(js_name = insertReference)]
pub fn web_insert(
    project: &[u8],
    project_name: &str,
    reference: &[u8],
    reference_name: &str,
    settings: &Settings,
) -> Result<Insertion, JsError> {
    insert_reference(
        &Source {
            project,
            project_name,
            reference,
            reference_name,
        },
        settings,
    )
    .map_err(js_error)
}

#[wasm_bindgen(js_name = inspectProject)]
pub fn web_inspect(
    project: &[u8],
    project_name: &str,
    empty_id: u8,
    phrase_id: u8,
) -> Result<Vec<String>, JsError> {
    let project = Project::parse(project, project_name).map_err(js_error)?;
    let options = Options::for_inspect(empty_id, phrase_id);
    Ok(inspect_project(&project, &options))
}
