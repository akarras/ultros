use crate::components::icon::Icon;
use crate::components::meta::{MetaDescription, MetaTitle};
use icondata as i;
use leptos::prelude::*;

#[component]
pub fn About() -> impl IntoView {
    view! {
        <div class="container mx-auto space-y-6">
            <MetaTitle title="About - Ultros" />
            <MetaDescription text="About Ultros - FFXIV Market Board Analysis Tool" />

            // Hero / Intro
            <div class="panel p-8 rounded-xl flex flex-col items-center text-center space-y-4">
                <h1 class="text-4xl font-bold text-[color:var(--brand-fg)]">"About Ultros"</h1>
                <p class="text-lg text-[color:var(--color-text)] max-w-3xl leading-relaxed">
                    "Ultros is a Final Fantasy XIV market board analysis tool that utilizes data sourced from Universalis.
                    Our goal is to help you make better decisions on the market board, whether you are a crafter, gatherer, or flipper."
                </p>
                <div class="flex flex-wrap justify-center gap-4 mt-4">
                    <a
                        href="https://discord.gg/pgdq9nGUP2"
                        class="btn btn-primary gap-2"
                        target="_blank"
                        rel="noopener noreferrer"
                    >
                        <Icon icon=i::BsDiscord width="1.2em" height="1.2em" />
                        "Join Discord"
                    </a>
                    <a
                        href="https://github.com/akarras/ultros"
                        class="btn btn-secondary gap-2"
                        target="_blank"
                        rel="noopener noreferrer"
                    >
                        <Icon icon=i::IoLogoGithub width="1.2em" height="1.2em" />
                        "View on GitHub"
                    </a>
                </div>
            </div>

            // Tech Stack
            <div class="panel p-6 rounded-xl">
                <h2 class="text-2xl font-bold mb-4 text-[color:var(--brand-fg)]">"Technology"</h2>
                <p class="text-[color:var(--color-text)] mb-4">
                    "Ultros is built with Rust for high performance and reliability. The project is open source and we welcome contributions."
                </p>
                <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
                    <TechCard
                        name="Axum"
                        desc="Backend web framework"
                        link="https://github.com/tokio-rs/axum"
                    />
                    <TechCard
                        name="Leptos"
                        desc="Full-stack Rust web framework"
                        link="https://github.com/leptos-rs/leptos"
                    />
                    <TechCard
                        name="SeaORM"
                        desc="Async ORM for the database"
                        link="https://github.com/SeaQL/sea-orm"
                    />
                    <TechCard
                        name="Serenity"
                        desc="Discord bot library"
                        link="https://github.com/serenity-rs/serenity"
                    />
                </div>
            </div>

            // Credits
            <div class="panel p-6 rounded-xl">
                <h2 class="text-2xl font-bold mb-4 text-[color:var(--brand-fg)]">"Credits & Acknowledgements"</h2>
                <div class="space-y-4 text-[color:var(--color-text)]">
                    <p>
                        "This project would not be possible without "
                        <a
                            href="https://universalis.app/"
                            class="text-[color:var(--brand-fg)] hover:underline"
                            target="_blank"
                            rel="noopener noreferrer"
                        >
                            "Universalis"
                        </a>
                        ", which provides the market board data. Please consider contributing to Universalis to help this site stay up to date."
                    </p>
                    <p>
                        "Game data is sourced from "
                        <a
                            href="https://github.com/xivapi/ffxiv-datamining"
                            class="text-[color:var(--brand-fg)] hover:underline"
                            target="_blank"
                            rel="noopener noreferrer"
                        >
                            "ffxiv-datamining"
                        </a>
                        "."
                    </p>
                    <p class="text-sm text-[color:var(--color-text-muted)] mt-8">
                        "FINAL FANTASY XIV © 2010 - 2024 SQUARE ENIX CO., LTD. All Rights Reserved."
                    </p>
                </div>
            </div>
        </div>
    }
}

#[component]
fn TechCard(name: &'static str, desc: &'static str, link: &'static str) -> impl IntoView {
    view! {
        <a
            href=link
            target="_blank"
            rel="noopener noreferrer"
            class="block p-4 rounded-lg border border-[color:var(--color-outline)] hover:border-[color:var(--brand-fg)] transition-colors"
        >
            <h3 class="font-bold text-lg text-[color:var(--brand-fg)]">{name}</h3>
            <p class="text-sm text-[color:var(--color-text-muted)]">{desc}</p>
        </a>
    }
}
