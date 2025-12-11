// frontend/src/lib.rs - Updated to use shared types

pub mod api;
pub mod components;

// Re-export shared types for frontend
pub use ai_agent_shared::*;

// frontend/Cargo.toml additions:
// [dependencies]
// ai-agent-shared = { path = "../shared", features = ["frontend"] }

// ============================================================================
// Example: Updated components using shared types
// ============================================================================

// frontend/src/components/chat.rs (excerpt)

use ai_agent_shared::{
    AgentRequest, FileSize, Language, MessageRole, NodeData, NodeType, StreamEvent, TreeNode,
};
use leptos::*;

#[component]
pub fn ChatInterface() -> impl IntoView {
    // Context using shared types
    let user_id = expect_context::<uuid::Uuid>();
    let chat_id = expect_context::<uuid::Uuid>();
    let session_id = expect_context::<String>();
    let language = expect_context::<Language>(); // Using shared Language type

    let (messages, set_messages) = create_signal(Vec::<ChatMessageUI>::new());
    let (input_text, set_input_text) = create_signal(String::new());
    let (tree_data, set_tree_data) = create_signal(Option::<TreeNode>::None);
    let (selected_nodes, set_selected_nodes) = create_signal(Vec::<uuid::Uuid>::new());

    // Send message using shared AgentRequest
    let send_message = create_action(move |message: &String| {
        let message = message.clone();
        let request = AgentRequest::new(
            message.clone(),
            chat_id,
            user_id,
            session_id.clone(),
            language.code().to_string(),
        )
        .with_tree_context(selected_nodes.get());

        async move {
            match send_agent_request(request).await {
                Ok(events) => {
                    // Handle stream events
                    for event in events {
                        match event {
                            StreamEvent::TextChunk { content } => {
                                log::info!("Received: {}", content);
                            }
                            StreamEvent::Error { error } => {
                                log::error!("Error: {}", error);
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    log::error!("Request failed: {}", e);
                }
            }
        }
    });

    view! {
        <div class="chat-container">
            <LanguageSelector current=language/>
            <TreeView node=tree_data/>
            <MessageList messages=messages/>
            <InputArea on_send=send_message/>
        </div>
    }
}

// Language selector using shared Language type
#[component]
fn LanguageSelector(current: Language) -> impl IntoView {
    let languages = vec![
        Language::English,
        Language::Ukrainian,
        Language::Russian,
        Language::German,
        Language::French,
        Language::Spanish,
    ];

    view! {
        <select class="language-selector">
            <For
                each=move || languages.clone()
                key=|lang| lang.code()
                children=move |lang| {
                    view! {
                        <option
                            value=lang.code()
                            selected=move || lang == current
                        >
                            {lang.name()}
                        </option>
                    }
                }
            />
        </select>
    }
}

// ============================================================================
// Example: Tree view with shared types
// ============================================================================

#[component]
fn TreeView(node: ReadSignal<Option<TreeNode>>) -> impl IntoView {
    view! {
        {move || node.get().map(|tree| view! {
            <div class="tree-view">
                <TreeNodeView node=tree/>
            </div>
        })}
    }
}

#[component]
fn TreeNodeView(node: TreeNode) -> impl IntoView {
    let (expanded, set_expanded) = create_signal(true);

    // Use shared NodeData pattern matching
    let node_info = match &node.data {
        NodeData::Root { title } => view! {
            <div class="node-root">
                <span class="icon">"🌳"</span>
                <span class="title">{title}</span>
            </div>
        }
        .into_view(),

        NodeData::Branch { label, description } => view! {
            <div class="node-branch">
                <span class="icon">"📁"</span>
                <span class="label">{label}</span>
                {description.as_ref().map(|d| view! {
                    <span class="description">{d}</span>
                })}
            </div>
        }
        .into_view(),

        NodeData::Image {
            url,
            size,
            mime_type,
            description,
            ..
        } => view! {
            <div class="node-image">
                <img src=url alt="Image" class="thumbnail"/>
                {size.map(|s| {
                    let file_size = FileSize::from(s);
                    view! {
                        <span class="size">{file_size.human_readable()}</span>
                    }
                })}
                {mime_type.as_ref().map(|m| view! {
                    <span class="mime">{m}</span>
                })}
                {description.as_ref().map(|d| view! {
                    <span class="description">{d}</span>
                })}
            </div>
        }
        .into_view(),
    };

    view! {
        <div class="tree-node">
            <div class="node-header" on:click=move |_| set_expanded.update(|e| *e = !*e)>
                {node_info}
                {move || if node.has_children() {
                    if expanded.get() { "▼" } else { "▶" }
                } else {
                    ""
                }}
            </div>

            {move || if expanded.get() && node.has_children() {
                view! {
                    <div class="node-children">
                        <For
                            each=move || node.children.clone()
                            key=|child| child.id
                            children=|child| view! {
                                <TreeNodeView node=child/>
                            }
                        />
                    </div>
                }.into_view()
            } else {
                view! { <></> }.into_view()
            }}
        </div>
    }
}

// ============================================================================
// Example: API client using shared types
// ============================================================================

// frontend/src/api/client.rs

use ai_agent_shared::{AgentRequest, ErrorResponse, Result, StreamEvent, TreeNode, UploadResponse};
use gloo_net::http::Request;

pub async fn send_agent_request(request: AgentRequest) -> Result<Vec<StreamEvent>> {
    let response = Request::post("/api/agent/chat")
        .json(&request)
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?
        .send()
        .await
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?;

    if !response.ok() {
        let error: ErrorResponse = response
            .json()
            .await
            .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?;
        return Err(error.error);
    }

    // Parse SSE stream (simplified)
    let events = vec![]; // Implement proper SSE parsing
    Ok(events)
}

pub async fn upload_image(file: web_sys::File) -> Result<UploadResponse> {
    use wasm_bindgen::JsCast;

    let form_data = web_sys::FormData::new()
        .map_err(|_| ai_agent_shared::AppError::internal("FormData creation failed"))?;

    form_data
        .append_with_blob("image", &file)
        .map_err(|_| ai_agent_shared::AppError::internal("Failed to append file"))?;

    let response = Request::post("/api/images/upload")
        .body(form_data)
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?
        .send()
        .await
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?;

    if !response.ok() {
        let error: ErrorResponse = response
            .json()
            .await
            .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?;
        return Err(error.error);
    }

    response
        .json::<UploadResponse>()
        .await
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))
}

pub async fn load_tree(user_id: uuid::Uuid, root_id: uuid::Uuid) -> Result<TreeNode> {
    let response = Request::get(&format!("/api/agent/tree/{}/{}", user_id, root_id))
        .send()
        .await
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?;

    if !response.ok() {
        let error: ErrorResponse = response
            .json()
            .await
            .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?;
        return Err(error.error);
    }

    response
        .json::<TreeNode>()
        .await
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))
}

pub async fn get_user_stats(user_id: uuid::Uuid) -> Result<ai_agent_shared::UserStats> {
    let response = Request::get(&format!("/api/stats/user/{}", user_id))
        .send()
        .await
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))?;

    if !response.ok() {
        return Err(ai_agent_shared::AppError::internal("Failed to load stats"));
    }

    response
        .json()
        .await
        .map_err(|e| ai_agent_shared::AppError::internal(e.to_string()))
}

// ============================================================================
// Example: Error handling in components
// ============================================================================

#[component]
pub fn ErrorBoundary(
    #[prop(into)] error: Signal<Option<ai_agent_shared::AppError>>,
    children: Children,
) -> impl IntoView {
    view! {
        {move || {
            if let Some(err) = error.get() {
                view! {
                    <div class="error-boundary">
                        <div class="error-icon">"⚠️"</div>
                        <div class="error-message">{err.message}</div>
                        <div class="error-code">{err.code.to_string()}</div>
                        {err.details.as_ref().map(|d| view! {
                            <pre class="error-details">
                                {serde_json::to_string_pretty(d).unwrap()}
                            </pre>
                        })}
                    </div>
                }.into_view()
            } else {
                children().into_view()
            }
        }}
    }
}

// ============================================================================
// Example: Validation in forms
// ============================================================================

use ai_agent_shared::{ValidationError, ValidationErrors};

#[component]
pub fn ImageUploadForm() -> impl IntoView {
    let (selected_file, set_selected_file) = create_signal(Option::<web_sys::File>::None);
    let (validation_errors, set_validation_errors) = create_signal(ValidationErrors::new());
    let (uploading, set_uploading) = create_signal(false);

    let validate_file = move |file: &web_sys::File| -> ValidationErrors {
        let mut errors = ValidationErrors::new();

        // Size validation using shared FileSize
        let max_size = ai_agent_shared::FileSize::megabytes(10);
        if file.size() as u64 > max_size.as_bytes() {
            errors.add(ValidationError::new(
                "file",
                format!("File too large. Max size: {}", max_size),
            ));
        }

        // Type validation using shared MimeType
        let mime = ai_agent_shared::MimeType::from(file.type_());
        if !mime.is_image() {
            errors.add(ValidationError::new("file", "Only image files are allowed"));
        }

        errors
    };

    let on_file_change = move |ev: web_sys::Event| {
        let input = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok());
        if let Some(input) = input {
            if let Some(files) = input.files() {
                if let Some(file) = files.get(0) {
                    let errors = validate_file(&file);
                    set_validation_errors.set(errors.clone());

                    if errors.is_empty() {
                        set_selected_file.set(Some(file));
                    } else {
                        set_selected_file.set(None);
                    }
                }
            }
        }
    };

    let on_submit = move |_| {
        if let Some(file) = selected_file.get() {
            set_uploading.set(true);
            spawn_local(async move {
                match upload_image(file).await {
                    Ok(response) => {
                        log::info!("Uploaded: {}", response.url);
                        set_selected_file.set(None);
                    }
                    Err(e) => {
                        log::error!("Upload failed: {}", e);
                    }
                }
                set_uploading.set(false);
            });
        }
    };

    view! {
        <form class="upload-form" on:submit=|e| e.prevent_default()>
            <input
                type="file"
                accept="image/*"
                on:change=on_file_change
                disabled=uploading
            />

            {move || {
                let errors = validation_errors.get();
                if !errors.is_empty() {
                    view! {
                        <div class="validation-errors">
                            <For
                                each=move || errors.errors.clone()
                                key=|err| err.field.clone()
                                children=|err| view! {
                                    <div class="validation-error">
                                        <span class="field">{err.field}</span>
                                        ": "
                                        <span class="message">{err.message}</span>
                                    </div>
                                }
                            />
                        </div>
                    }.into_view()
                } else {
                    view! { <></> }.into_view()
                }
            }}

            <button
                type="button"
                on:click=on_submit
                disabled=move || selected_file.get().is_none() || uploading.get()
            >
                {move || if uploading.get() { "Uploading..." } else { "Upload" }}
            </button>
        </form>
    }
}
