//! The one screen this slice renders.
//!
//! Deliberately unstyled and structurally plain. The point of the slice is that
//! a browser reaches the panel, authenticates, receives a typed document and
//! displays fields from it; anything spent on layout before that works is spent
//! on something unproven.

use leptos::prelude::*;
use panel_domain::schemas::SchemaId;
use panel_domain::ui::UiBootstrapSnapshot;

use crate::api;

#[component]
pub fn App() -> impl IntoView {
    let (token, set_token) = signal(Option::<String>::None);
    let (snapshot, set_snapshot) = signal(Option::<UiBootstrapSnapshot>::None);
    let (error, set_error) = signal(Option::<String>::None);

    view! {
        <main>
            <h1>"Hydra"</h1>
            {move || match snapshot.get() {
                Some(document) => view! { <Overview document=document /> }.into_any(),
                None => view! {
                    <Login
                        set_token=set_token
                        set_snapshot=set_snapshot
                        set_error=set_error
                    />
                }
                    .into_any(),
            }}
            {move || {
                error
                    .get()
                    .map(|message| view! { <p class="error" role="alert">{message}</p> })
            }}
            {move || token.get().map(|_| view! { <p>"Session established."</p> })}
        </main>
    }
}

#[component]
fn Login(
    set_token: WriteSignal<Option<String>>,
    set_snapshot: WriteSignal<Option<UiBootstrapSnapshot>>,
    set_error: WriteSignal<Option<String>>,
) -> impl IntoView {
    let (username, set_username) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (busy, set_busy) = signal(false);

    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        set_error.set(None);
        set_busy.set(true);

        leptos::task::spawn_local(async move {
            match api::login(username.get_untracked(), password.get_untracked()).await {
                Ok(success) => {
                    // The bootstrap request follows the login immediately: a
                    // token that cannot fetch the first document is not a
                    // working session, and showing a signed-in shell before
                    // knowing that would be a lie the operator has to discover.
                    match api::bootstrap(&success.token).await {
                        Ok(document) => {
                            set_token.set(Some(success.token));
                            set_snapshot.set(Some(document));
                        }
                        Err(failure) => set_error.set(Some(failure.to_string())),
                    }
                }
                Err(failure) => set_error.set(Some(failure.to_string())),
            }
            set_busy.set(false);
        });
    };

    view! {
        <form on:submit=submit>
            <label>
                "Username"
                <input
                    type="text"
                    autocomplete="username"
                    prop:value=move || username.get()
                    on:input=move |event| set_username.set(event_target_value(&event))
                />
            </label>
            <label>
                "Password"
                <input
                    type="password"
                    autocomplete="current-password"
                    prop:value=move || password.get()
                    on:input=move |event| set_password.set(event_target_value(&event))
                />
            </label>
            <button type="submit" disabled=move || busy.get()>
                {move || if busy.get() { "Signing in…" } else { "Sign in" }}
            </button>
        </form>
    }
}

#[component]
fn Overview(document: UiBootstrapSnapshot) -> impl IntoView {
    // The version the panel sent against the version this binary was compiled
    // against. Both come from the same registry, so a mismatch means the two
    // halves were built from different commits — which is the case
    // `ui_contracts` was raised for. Reported rather than guessed at.
    let compiled_against = SchemaId::UiBootstrap.version();
    let served = document.schema_version;

    view! {
        <section>
            <h2>"Signed in as " {document.admin.username.clone()}</h2>

            {(served != compiled_against)
                .then(|| {
                    view! {
                        <p class="error" role="alert">
                            {format!(
                                "The panel serves ui_bootstrap version {served}, this frontend was \
                                 built against {compiled_against}. Update the frontend.",
                            )}
                        </p>
                    }
                })}

            <dl>
                <dt>"Users"</dt>
                <dd>
                    {document.users.total} " total, " {document.users.active} " active"
                </dd>

                <dt>"Nodes"</dt>
                <dd>
                    {document.nodes.total} " total, " {document.nodes.healthy} " healthy, "
                    {document.nodes.offline} " offline"
                </dd>

                <dt>"Clusters"</dt>
                <dd>{document.clusters.total} " total"</dd>

                // Host memory against host total, never against the budget.
                //
                // The budget bounds the panel *process*, and its resident size
                // lives in the resource-budget document rather than this one.
                // Rendering host usage against it read as a 1690x overrun on a
                // machine that was fine, and would have read wrong even after
                // the units were fixed: two different quantities side by side.
                <dt>"Host memory"</dt>
                <dd>
                    {document.system.memory_used_bytes / 1_048_576} " MB used of "
                    {document.system.memory_total_bytes / 1_048_576} " MB"
                </dd>

                <dt>"Panel process budget"</dt>
                <dd>{document.system.memory_budget_mb} " MB"</dd>

                <dt>"Active alerts"</dt>
                <dd>{document.system.active_alerts.len()}</dd>
            </dl>
        </section>
    }
}
