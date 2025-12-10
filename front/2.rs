// Frontend main.rs for Leptos SSR

use leptos::*;
use leptos_meta::*;
use leptos_router::*;

mod components;
use components::{ChatInterface, EnhancedChatStyles};

// ============================================================================
// Main App Component
// ============================================================================

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/app.css"/>
        <EnhancedChatStyles/>

        <Title text="AI Agent Chat"/>
        <Meta name="description" content="AI Agent with Multimodal Support"/>

        <Router>
            <Routes>
                <Route path="/" view=HomePage/>
                <Route path="/chat/:chat_id" view=ChatPage/>
                <Route path="/*any" view=NotFound/>
            </Routes>
        </Router>
    }
}

// ============================================================================
// Home Page
// ============================================================================

#[component]
fn HomePage() -> impl IntoView {
    view! {
        <div class="home-container">
            <h1>"AI Agent Chat"</h1>
            <p>"Multimodal AI assistant with image analysis"</p>
            <a href="/chat/new" class="start-button">
                "Start New Chat"
            </a>
        </div>
    }
}

// ============================================================================
// Chat Page with Context
// ============================================================================

#[component]
fn ChatPage() -> impl IntoView {
    let params = use_params_map();
    let chat_id = move || {
        params.with(|p| {
            p.get("chat_id")
                .and_then(|id| {
                    if id == "new" {
                        Some(uuid::Uuid::new_v4())
                    } else {
                        uuid::Uuid::parse_str(id).ok()
                    }
                })
                .unwrap_or_else(uuid::Uuid::new_v4)
        })
    };

    // Extract from cookies or headers (set by server)
    let user_id = extract_user_id();
    let session_id = extract_session_id();
    let language = extract_language();

    // Provide contexts
    provide_context(user_id);
    provide_context(chat_id());
    provide_context(session_id);
    provide_context(language);

    view! {
        <ChatInterface/>
    }
}

// ============================================================================
// Context Extractors (from SSR)
// ============================================================================

#[cfg(feature = "ssr")]
fn extract_user_id() -> uuid::Uuid {
    use leptos_axum::extract;

    extract(|req: axum::http::Request<axum::body::Body>| async move {
        req.extensions()
            .get::<uuid::Uuid>()
            .cloned()
            .unwrap_or_else(uuid::Uuid::new_v4)
    })
    .unwrap_or_else(|_| uuid::Uuid::new_v4())
}

#[cfg(not(feature = "ssr"))]
fn extract_user_id() -> uuid::Uuid {
    // Client-side: get from local storage or cookie
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item("user_id").ok().flatten())
        .and_then(|id| uuid::Uuid::parse_str(&id).ok())
        .unwrap_or_else(uuid::Uuid::new_v4)
}

#[cfg(feature = "ssr")]
fn extract_session_id() -> String {
    use leptos_axum::extract;

    extract(|req: axum::http::Request<axum::body::Body>| async move {
        req.extensions()
            .get::<String>()
            .cloned()
            .unwrap_or_else(|| "default".to_string())
    })
    .unwrap_or_else(|_| "default".to_string())
}

#[cfg(not(feature = "ssr"))]
fn extract_session_id() -> String {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item("session_id").ok().flatten())
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(feature = "ssr")]
fn extract_language() -> String {
    use leptos_axum::extract;

    extract(|req: axum::http::Request<axum::body::Body>| async move {
        req.extensions()
            .get::<String>()
            .cloned()
            .unwrap_or_else(|| "en".to_string())
    })
    .unwrap_or_else(|_| "en".to_string())
}

#[cfg(not(feature = "ssr"))]
fn extract_language() -> String {
    web_sys::window()
        .and_then(|w| w.navigator().language())
        .unwrap_or_else(|| "en".to_string())
}

// ============================================================================
// Not Found Page
// ============================================================================

#[component]
fn NotFound() -> impl IntoView {
    view! {
        <div class="not-found">
            <h1>"404 - Page Not Found"</h1>
            <a href="/">"Go Home"</a>
        </div>
    }
}

// ============================================================================
// Server-Side Main (SSR)
// ============================================================================

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use axum::Router;
    use leptos::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use tower_http::services::ServeDir;

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Get Leptos configuration
    let conf = get_configuration(None).await.unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    log::info!("Starting Leptos SSR server on {}", addr);

    // Create Axum router
    let app = Router::new()
        .leptos_routes(&leptos_options, routes, App)
        .fallback(leptos_axum::file_and_error_handler(App))
        .nest_service("/pkg", ServeDir::new("./pkg"))
        .with_state(leptos_options);

    // Start server
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    log::info!("🎨 Leptos UI listening on http://{}", addr);

    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

// ============================================================================
// Client-Side Main (Hydration)
// ============================================================================

#[cfg(not(feature = "ssr"))]
pub fn main() {
    use leptos::*;

    console_error_panic_hook::set_once();

    mount_to_body(|| {
        view! { <App/> }
    });
}
