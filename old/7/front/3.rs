use leptos::html::Input;
use leptos::*;
use wasm_bindgen::JsCast;
use web_sys::{File, FormData};

// ============================================================================
// Enhanced Chat Interface with Image Upload
// ============================================================================

#[component]
pub fn ChatInterface() -> impl IntoView {
    let user_id = expect_context::<Uuid>();
    let chat_id = expect_context::<Uuid>();
    let session_id = expect_context::<String>();
    let language = expect_context::<String>();

    let (messages, set_messages) = create_signal(Vec::<ChatMessageUI>::new());
    let (input_text, set_input_text) = create_signal(String::new());
    let (tree_data, set_tree_data) = create_signal(Option::<TreeNode>::None);
    let (selected_nodes, set_selected_nodes) = create_signal(Vec::<Uuid>::new());
    let (is_loading, set_is_loading) = create_signal(false);
    let (current_response, set_current_response) = create_signal(String::new());
    let (upload_progress, set_upload_progress) = create_signal(Option::<f64>::None);

    // File input ref
    let file_input_ref = create_node_ref::<Input>();

    // Send message action
    let send_message = create_action(move |message: &String| {
        let message = message.clone();
        let user_id_val = user_id;
        let chat_id_val = chat_id;
        let session_id_val = session_id.clone();
        let language_val = language.clone();
        let selected = selected_nodes.get();

        async move {
            set_is_loading.set(true);
            set_current_response.set(String::new());

            let user_msg = ChatMessageUI {
                id: Uuid::new_v4(),
                role: "user".to_string(),
                content: message.clone(),
                tree_refs: selected.clone(),
                timestamp: js_sys::Date::new_0().to_iso_string().as_string().unwrap(),
            };
            set_messages.update(|msgs| msgs.push(user_msg));

            let request = AgentRequest {
                message: message.clone(),
                chat_id: chat_id_val,
                user_id: user_id_val,
                session_id: session_id_val,
                language: language_val,
                tree_context: if selected.is_empty() {
                    None
                } else {
                    Some(selected)
                },
            };

            match stream_agent_response(request, set_current_response).await {
                Ok(_) => {
                    let assistant_msg = ChatMessageUI {
                        id: Uuid::new_v4(),
                        role: "assistant".to_string(),
                        content: current_response.get(),
                        tree_refs: vec![],
                        timestamp: js_sys::Date::new_0().to_iso_string().as_string().unwrap(),
                    };
                    set_messages.update(|msgs| msgs.push(assistant_msg));
                    set_current_response.set(String::new());
                }
                Err(e) => {
                    log::error!("Stream error: {}", e);
                }
            }

            set_is_loading.set(false);
        }
    });

    // Upload images action
    let upload_images = create_action(move |files: &Vec<File>| {
        let files = files.clone();
        let user_id_val = user_id;

        async move {
            set_upload_progress.set(Some(0.0));

            match upload_images_to_s3(files, user_id_val, set_upload_progress).await {
                Ok(uploaded_nodes) => {
                    log::info!("Uploaded {} images", uploaded_nodes.len());

                    // Reload tree to show new images
                    if let Ok(tree) = load_tree(user_id_val).await {
                        set_tree_data.set(Some(tree));
                    }

                    set_upload_progress.set(None);
                }
                Err(e) => {
                    log::error!("Upload failed: {}", e);
                    set_upload_progress.set(None);
                }
            }
        }
    });

    // Handle file selection
    let on_file_select = move |_| {
        if let Some(input) = file_input_ref.get() {
            if let Some(files) = input.files() {
                let file_vec: Vec<File> =
                    (0..files.length()).filter_map(|i| files.get(i)).collect();

                if !file_vec.is_empty() {
                    upload_images.dispatch(file_vec);
                }
            }
        }
    };

    let on_send = move |_| {
        let text = input_text.get();
        if !text.trim().is_empty() {
            send_message.dispatch(text);
            set_input_text.set(String::new());
        }
    };

    create_effect(move |_| {
        spawn_local(async move {
            if let Ok(tree) = load_tree(user_id).await {
                set_tree_data.set(Some(tree));
            }
        });
    });

    view! {
        <div class="chat-container">
            <div class="chat-layout">
                <div class="tree-panel">
                    <div class="tree-header">
                        <h3>"Object Tree"</h3>
                        <button
                            class="upload-button"
                            on:click=move |_| {
                                if let Some(input) = file_input_ref.get() {
                                    input.click();
                                }
                            }
                            disabled=move || upload_progress.get().is_some()
                        >
                            {move || {
                                if let Some(progress) = upload_progress.get() {
                                    format!("Uploading... {:.0}%", progress * 100.0)
                                } else {
                                    "📤 Upload Images".to_string()
                                }
                            }}
                        </button>
                        <input
                            type="file"
                            multiple
                            accept="image/*"
                            style="display: none"
                            node_ref=file_input_ref
                            on:change=on_file_select
                        />
                    </div>

                    {move || tree_data.get().map(|tree| view! {
                        <TreeView
                            node=tree
                            selected=selected_nodes
                            on_select=move |id| {
                                set_selected_nodes.update(|nodes| {
                                    if nodes.contains(&id) {
                                        nodes.retain(|n| *n != id);
                                    } else {
                                        nodes.push(id);
                                    }
                                });
                            }
                        />
                    })}
                </div>

                <div class="chat-panel">
                    <div class="messages-container">
                        <For
                            each=move || messages.get()
                            key=|msg| msg.id
                            children=move |msg| view! {
                                <MessageBubble message=msg />
                            }
                        />

                        {move || {
                            let response = current_response.get();
                            if !response.is_empty() {
                                view! {
                                    <div class="message assistant streaming">
                                        <div class="message-content">{response}</div>
                                        <div class="typing-indicator">
                                            <span></span>
                                            <span></span>
                                            <span></span>
                                        </div>
                                    </div>
                                }.into_view()
                            } else {
                                view! { <></> }.into_view()
                            }
                        }}
                    </div>

                    <div class="input-container">
                        {move || {
                            if !selected_nodes.get().is_empty() {
                                view! {
                                    <div class="selected-nodes-indicator">
                                        <span>
                                            "🖼️ Selected: " {selected_nodes.get().len()} " images"
                                        </span>
                                        <button on:click=move |_| set_selected_nodes.set(vec![])>
                                            "Clear"
                                        </button>
                                    </div>
                                }.into_view()
                            } else {
                                view! { <></> }.into_view()
                            }
                        }}

                        <div class="input-row">
                            <textarea
                                prop:value=input_text
                                on:input=move |ev| set_input_text.set(event_target_value(&ev))
                                on:keydown=move |ev| {
                                    if ev.key() == "Enter" && !ev.shift_key() {
                                        ev.prevent_default();
                                        on_send(());
                                    }
                                }
                                placeholder="Type your message... (Shift+Enter for new line)"
                                disabled=is_loading
                            />
                            <button
                                on:click=on_send
                                disabled=move || is_loading.get() || input_text.get().trim().is_empty()
                                class="send-button"
                            >
                                {move || if is_loading.get() { "..." } else { "➤" }}
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}

// ============================================================================
// Enhanced TreeView with Image Previews
// ============================================================================

#[component]
fn TreeView(
    node: TreeNode,
    selected: ReadSignal<Vec<Uuid>>,
    on_select: impl Fn(Uuid) + 'static + Clone,
) -> impl IntoView {
    let node_id = node.id;
    let is_selected = move || selected.get().contains(&node_id);
    let on_select_clone = on_select.clone();

    view! {
        <div class="tree-node">
            <div
                class=move || format!(
                    "node-header {} {}",
                    match node.node_type {
                        NodeType::Root => "root",
                        NodeType::Branch => "branch",
                        NodeType::ImageLeaf => "leaf",
                    },
                    if is_selected() { "selected" } else { "" }
                )
                on:click=move |_| on_select_clone(node_id)
            >
                {match &node.data {
                    NodeData::Root { title } => view! {
                        <div class="node-content">
                            <span class="node-icon">"🌳"</span>
                            <span class="node-label">{title}</span>
                        </div>
                    }.into_view(),
                    NodeData::Branch { label, description } => view! {
                        <div class="node-content">
                            <span class="node-icon">"📁"</span>
                            <div class="node-text">
                                <span class="node-label">{label}</span>
                                {description.as_ref().map(|d| view! {
                                    <span class="node-description">{d}</span>
                                })}
                            </div>
                        </div>
                    }.into_view(),
                    NodeData::Image { url, description, size, .. } => view! {
                        <div class="node-content image-node">
                            <div class="image-wrapper">
                                <img
                                    src=url
                                    alt="Node image"
                                    class="node-thumbnail"
                                    loading="lazy"
                                />
                                {size.map(|s| view! {
                                    <span class="image-size">
                                        {format_bytes(*s)}
                                    </span>
                                })}
                            </div>
                            {description.as_ref().map(|d| view! {
                                <span class="node-description">{d}</span>
                            })}
                        </div>
                    }.into_view(),
                }}
            </div>

            {if !node.children.is_empty() {
                view! {
                    <div class="node-children">
                        <For
                            each=move || node.children.clone()
                            key=|child| child.id
                            children=move |child| {
                                let on_select_inner = on_select.clone();
                                view! {
                                    <TreeView
                                        node=child
                                        selected=selected
                                        on_select=on_select_inner
                                    />
                                }
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
// API Functions
// ============================================================================

use gloo_net::http::Request;
use wasm_bindgen_futures::JsFuture;

async fn stream_agent_response(
    request: AgentRequest,
    set_current: WriteSignal<String>,
) -> Result<(), String> {
    let response = Request::post("/api/agent/chat")
        .json(&request)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    // Use EventSource for SSE
    let event_source =
        web_sys::EventSource::new("/api/agent/chat").map_err(|e| format!("{:?}", e))?;

    let on_message = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        if let Ok(data) = e.data().dyn_into::<js_sys::JsString>() {
            let text: String = data.into();

            if let Ok(event) = serde_json::from_str::<StreamEvent>(&text) {
                match event {
                    StreamEvent::TextChunk { content } => {
                        set_current.update(|s| s.push_str(&content));
                    }
                    StreamEvent::Complete { .. } => {
                        event_source.close();
                    }
                    _ => {}
                }
            }
        }
    }) as Box<dyn FnMut(_)>);

    event_source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    Ok(())
}

async fn upload_images_to_s3(
    files: Vec<File>,
    user_id: Uuid,
    set_progress: WriteSignal<Option<f64>>,
) -> Result<Vec<UploadResponse>, String> {
    let mut responses = Vec::new();
    let total = files.len() as f64;

    for (i, file) in files.into_iter().enumerate() {
        let form_data = FormData::new().map_err(|e| format!("{:?}", e))?;
        form_data
            .append_with_blob("image", &file)
            .map_err(|e| format!("{:?}", e))?;

        let request = Request::post("/api/images/upload")
            .body(form_data)
            .map_err(|e| e.to_string())?;

        match request.send().await {
            Ok(response) => {
                if let Ok(upload_resp) = response.json::<UploadResponse>().await {
                    responses.push(upload_resp);
                }
            }
            Err(e) => {
                log::error!("Upload failed: {}", e);
            }
        }

        set_progress.set(Some((i + 1) as f64 / total));
    }

    Ok(responses)
}

async fn load_tree(user_id: Uuid) -> Result<TreeNode, String> {
    let root_id = Uuid::new_v4(); // Get from context or storage

    let response = Request::get(&format!("/api/agent/tree/{}/{}", user_id, root_id))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json::<TreeNode>().await.map_err(|e| e.to_string())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_idx])
}

// ============================================================================
// Enhanced CSS with Upload & Image Preview
// ============================================================================

#[component]
pub fn EnhancedChatStyles() -> impl IntoView {
    view! {
        <style>
            r#"
            * {
                box-sizing: border-box;
                margin: 0;
                padding: 0;
            }

            .chat-container {
                width: 100%;
                height: 100vh;
                overflow: hidden;
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            }

            .chat-layout {
                display: grid;
                grid-template-columns: 320px 1fr;
                height: 100%;
                gap: 0;
            }

            .tree-panel {
                border-right: 1px solid #e0e0e0;
                padding: 1rem;
                overflow-y: auto;
                background: #fafafa;
            }

            .tree-header {
                display: flex;
                justify-content: space-between;
                align-items: center;
                margin-bottom: 1rem;
                gap: 0.5rem;
            }

            .tree-header h3 {
                font-size: 1.1rem;
                font-weight: 600;
            }

            .upload-button {
                padding: 0.5rem 1rem;
                background: #1976d2;
                color: white;
                border: none;
                border-radius: 6px;
                cursor: pointer;
                font-size: 0.875rem;
                transition: all 0.2s;
                white-space: nowrap;
            }

            .upload-button:hover:not(:disabled) {
                background: #1565c0;
                transform: translateY(-1px);
            }

            .upload-button:disabled {
                background: #90caf9;
                cursor: wait;
            }

            .tree-node {
                margin-left: 0.75rem;
            }

            .node-header {
                padding: 0.625rem;
                margin: 0.25rem 0;
                border-radius: 6px;
                cursor: pointer;
                transition: all 0.2s;
                border: 2px solid transparent;
            }

            .node-header:hover {
                background: #e8e8e8;
            }

            .node-header.selected {
                background: #e3f2fd;
                border-color: #1976d2;
                box-shadow: 0 2px 4px rgba(25, 118, 210, 0.2);
            }

            .node-content {
                display: flex;
                align-items: center;
                gap: 0.5rem;
            }

            .image-node {
                flex-direction: column;
                align-items: flex-start;
            }

            .image-wrapper {
                position: relative;
                width: 100%;
            }

            .node-thumbnail {
                width: 100%;
                height: 120px;
                object-fit: cover;
                border-radius: 6px;
                border: 1px solid #e0e0e0;
            }

            .image-size {
                position: absolute;
                bottom: 4px;
                right: 4px;
                background: rgba(0, 0, 0, 0.7);
                color: white;
                padding: 2px 6px;
                border-radius: 3px;
                font-size: 0.7rem;
            }

            .node-text {
                display: flex;
                flex-direction: column;
                gap: 0.25rem;
            }

            .node-label {
                font-weight: 500;
                font-size: 0.9rem;
            }

            .node-description {
                font-size: 0.8rem;
                color: #666;
                margin-top: 0.25rem;
            }

            .node-children {
                margin-left: 1rem;
                border-left: 2px solid #e0e0e0;
                padding-left: 0.5rem;
            }

            .chat-panel {
                display: flex;
                flex-direction: column;
                height: 100%;
                background: white;
            }

            .messages-container {
                flex: 1;
                overflow-y: auto;
                padding: 1.5rem;
                display: flex;
                flex-direction: column;
                gap: 1rem;
            }

            .message {
                padding: 1rem;
                border-radius: 12px;
                max-width: 75%;
                animation: slideIn 0.2s ease-out;
            }

            @keyframes slideIn {
                from {
                    opacity: 0;
                    transform: translateY(10px);
                }
                to {
                    opacity: 1;
                    transform: translateY(0);
                }
            }

            .message.user {
                background: linear-gradient(135deg, #1976d2 0%, #1565c0 100%);
                color: white;
                align-self: flex-end;
                box-shadow: 0 2px 8px rgba(25, 118, 210, 0.3);
            }

            .message.assistant {
                background: #f5f5f5;
                color: #333;
                align-self: flex-start;
                border: 1px solid #e0e0e0;
            }

            .message.streaming {
                animation: pulse 1.5s infinite;
            }

            .typing-indicator {
                display: flex;
                gap: 4px;
                padding: 0.5rem 0;
            }

            .typing-indicator span {
                width: 8px;
                height: 8px;
                border-radius: 50%;
                background: #666;
                animation: bounce 1.4s infinite;
            }

            .typing-indicator span:nth-child(2) {
                animation-delay: 0.2s;
            }

            .typing-indicator span:nth-child(3) {
                animation-delay: 0.4s;
            }

            @keyframes bounce {
                0%, 60%, 100% { transform: translateY(0); }
                30% { transform: translateY(-10px); }
            }

            .input-container {
                border-top: 1px solid #e0e0e0;
                padding: 1rem;
                background: #fafafa;
            }

            .selected-nodes-indicator {
                background: #e3f2fd;
                padding: 0.75rem;
                border-radius: 8px;
                display: flex;
                justify-content: space-between;
                align-items: center;
                margin-bottom: 0.75rem;
                border: 1px solid #90caf9;
            }

            .selected-nodes-indicator button {
                padding: 0.375rem 0.75rem;
                background: #1976d2;
                color: white;
                border: none;
                border-radius: 4px;
                cursor: pointer;
                font-size: 0.875rem;
            }

            .input-row {
                display: flex;
                gap: 0.75rem;
            }

            textarea {
                flex: 1;
                padding: 0.875rem;
                border: 2px solid #e0e0e0;
                border-radius: 8px;
                resize: none;
                min-height: 80px;
                font-family: inherit;
                font-size: 0.95rem;
                transition: border-color 0.2s;
            }

            textarea:focus {
                outline: none;
                border-color: #1976d2;
            }

            .send-button {
                padding: 0 1.5rem;
                background: #1976d2;
                color: white;
                border: none;
                border-radius: 8px;
                cursor: pointer;
                font-size: 1.25rem;
                transition: all 0.2s;
            }

            .send-button:hover:not(:disabled) {
                background: #1565c0;
                transform: scale(1.05);
            }

            .send-button:disabled {
                background: #ccc;
                cursor: not-allowed;
                transform: scale(1);
            }

            @media (max-width: 768px) {
                .chat-layout {
                    grid-template-columns: 1fr;
                }

                .tree-panel {
                    max-height: 40vh;
                }
            }
            "#
        </style>
    }
}
