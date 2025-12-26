#[cfg(feature = "hydrate")]
use leptos::leptos_dom::helpers::set_timeout;

use leptos::prelude::*;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: Uuid,
    pub message: String,
    pub level: ToastLevel,
    pub duration: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Toasts(pub RwSignal<Vec<Toast>>);

impl Copy for Toasts {}

pub fn provide_toast_context() {
    provide_context(Toasts(RwSignal::new(Vec::new())));
}

pub fn use_toast() -> Option<Toasts> {
    use_context::<Toasts>()
}

impl Toasts {
    pub fn add(&self, message: impl Into<String>, level: ToastLevel, duration: Option<u64>) {
        let id = Uuid::new_v4();
        let toast = Toast {
            id,
            message: message.into(),
            level,
            duration,
        };
        self.0.update(|toasts| toasts.push(toast));

        if let Some(duration) = duration {
            let toasts = *self;
            #[cfg(feature = "hydrate")]
            set_timeout(
                move || {
                    toasts.remove(id);
                },
                std::time::Duration::from_millis(duration),
            );
            #[cfg(not(feature = "hydrate"))]
            {
                let _ = toasts;
                let _ = duration;
            }
        }
    }

    pub fn remove(&self, id: Uuid) {
        self.0.update(|toasts| {
            if let Some(index) = toasts.iter().position(|t| t.id == id) {
                toasts.remove(index);
            }
        });
    }

    #[allow(dead_code)]
    pub fn info(&self, message: impl Into<String>) {
        self.add(message, ToastLevel::Info, Some(3000));
    }

    pub fn success(&self, message: impl Into<String>) {
        self.add(message, ToastLevel::Success, Some(3000));
    }

    #[allow(dead_code)]
    pub fn warning(&self, message: impl Into<String>) {
        self.add(message, ToastLevel::Warning, Some(5000));
    }

    pub fn error(&self, message: impl Into<String>) {
        self.add(message, ToastLevel::Error, Some(5000));
    }
}
