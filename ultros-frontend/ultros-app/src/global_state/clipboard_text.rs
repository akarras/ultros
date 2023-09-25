use leptos::*;

#[derive(Clone, Copy)]
pub struct GlobalLastCopiedText(pub RwSignal<Option<String>>);
