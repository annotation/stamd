use crate::common::ApiError;
use stam::{AnnotationStore, Config, StamError};
use std::collections::HashMap;

pub struct MultiStore {
    dir: String,
    extension: String,
    stores: HashMap<String, AnnotationStore>,
    last_access: HashMap<String, usize>,
}

impl MultiStore {
    // Get an AnnotationStore by name
    //fn get(id: &str) -> Result<&AnnotationStore, ApiError> {}
}
