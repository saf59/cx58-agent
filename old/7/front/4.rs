use leptos::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use web_sys::{EventSource, MessageEvent};

// ============================================================================
// Domain Models (matching backend)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_type: NodeType,
    pub data: NodeData,
    pub children: Vec<TreeNode>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum NodeType {
    Root,
    Branch,
    ImageLeaf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NodeData {
    Root {
        title: String,
    },
    Branch {
        label: String,
        description: Option<String>,
    },
    Image {
        url: String,
        description: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub message: String,
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub session_id: String,
    pub language: String,
    pub tree_context: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    TextChunk { content: String },
    TreeUpdate { nodes: Vec<TreeNode> },
    ToolCall { tool: String, status: String },
    Complete { message_id: Uuid },
    Error { error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessageUI {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub tree_refs: Vec<Uuid>,
    pub timestamp: String,
}

// ============================================================================
// Main Chat Component
// ============================================================================

#[component]
pub fn ChatInterface() -> impl IntoView {
    // Context from SSR
    let user_id = expect_context::<Uuid>();
    let chat_id = expect_context::<Uuid>();
    let session_id = expect_context::<String>();
    let language = expect_context::<String>();

    // State
    let (messages, set_messages) = create_signal(Vec::<ChatMessageUI>::new());
    let (input_text, set_input_text) = create_signal(String::new());
    let (tree_data, set_tree_data) = create_signal(Option::<TreeNode>::None);
    let (selected_nodes, set_selected_nodes) = create_signal(Vec::<Uuid>::new());
    let (is_loading, set_is_loading) = create_signal(false);
    let (current_response, set_current_response) = create_signal(String::new());

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

            // Add user message
            let user_msg = ChatMessageUI {
                id: Uuid::new_v4(),
                role: "user".to_string(),
                content: message.clone(),
                tree_refs: selected.clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            set_messages.update(|msgs| msgs.push(user_msg));

            // Create request
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

            // Stream response
            match stream_agent_response(request).await {
                Ok(_) => {
                    // Add assistant message
                    let assistant_msg = ChatMessageUI {
                        id: Uuid::new_v4(),
                        role: "assistant".to_string(),
                        content: current_response.get(),
                        tree_refs: vec![],
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    };
                    set_messages.update(|msgs| msgs.push(assistant_msg));
                }
                Err(e) => {
                    log::error!("Stream error: {}", e);
                }
            }

            set_is_loading.set(false);
        }
    });

    // Handle send
    let on_send = move |_| {
        let text = input_text.get();
        if !text.trim().is_empty() {
            send_message.dispatch(text);
            set_input_text.set(String::new());
        }
    };

    // Load tree on mount
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
                // Tree Visualization Panel
                <div class="tree-panel">
                    <h3>"Object Tree"</h3>
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

                // Chat Panel
                <div class="chat-panel">
                    <div class="messages-container">
                        <For
                            each=move || messages.get()
                            key=|msg| msg.id
                            children=move |msg| view! {
                                <MessageBubble message=msg />
                            }
                        />

                        // Current streaming response
                        {move || {
                            let response = current_response.get();
                            if !response.is_empty() {
                                view! {
                                    <div class="message assistant streaming">
                                        <div class="message-content">{response}</div>
                                    </div>
                                }.into_view()
                            } else {
                                view! { <></> }.into_view()
                            }
                        }}
                    </div>

                    // Input area
                    <div class="input-container">
                        {move || {
                            if !selected_nodes.get().is_empty() {
                                view! {
                                    <div class="selected-nodes-indicator">
                                        "Selected: " {selected_nodes.get().len()} " nodes"
                                        <button on:click=move |_| set_selected_nodes.set(vec![])>
                                            "Clear"
                                        </button>
                                    </div>
                                }.into_view()
                            } else {
                                view! { <></> }.into_view()
                            }
                        }}

                        <textarea
                            prop:value=input_text
                            on:input=move |ev| set_input_text.set(event_target_value(&ev))
                            placeholder="Type your message..."
                            disabled=is_loading
                        />
                        <button
                            on:click=on_send
                            disabled=move || is_loading.get() || input_text.get().trim().is_empty()
                        >
                            {move || if is_loading.get() { "Sending..." } else { "Send" }}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}

// ============================================================================
// Tree Visualization Component
// ============================================================================

#[component]
fn TreeView(
    node: TreeNode,
    selected: ReadSignal<Vec<Uuid>>,
    on_select: impl Fn(Uuid) + 'static,
) -> impl IntoView {
    let node_id = node.id;
    let is_selected = move || selected.get().contains(&node_id);

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
                on:click=move |_| on_select(node_id)
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
                            <span class="node-label">{label}</span>
                            {description.as_ref().map(|d| view! {
                                <span class="node-description">{d}</span>
                            })}
                        </div>
                    }.into_view(),
                    NodeData::Image { url, description, .. } => view! {
                        <div class="node-content image-node">
                            <img src=url alt="Node image" class="node-thumbnail" />
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
                            children=move |child| view! {
                                <TreeView
                                    node=child
                                    selected=selected
                                    on_select=on_select
                                />
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
// Message Bubble Component
// ============================================================================

#[component]
fn MessageBubble(message: ChatMessageUI) -> impl IntoView {
    view! {
        <div class=format!("message {}", message.role)>
            <div class="message-header">
                <span class="message-role">{&message.role}</span>
                <span class="message-time">{&message.timestamp}</span>
            </div>
            <div class="message-content">
                {&message.content}
            </div>
            {if !message.tree_refs.is_empty() {
                view! {
                    <div class="message-refs">
                        "🔗 References " {message.tree_refs.len()} " nodes"
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

async fn stream_agent_response(request: AgentRequest) -> Result<(), String> {
    use gloo_net::http::Request;

    let response = Request::post("/api/agent/chat")
        .json(&request)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    // Handle SSE stream
    let reader = response.body().map_err(|e| format!("{:?}", e))?;

    // Parse SSE events
    // Note: In real implementation, use proper SSE parsing
    // This is simplified for demonstration

    Ok(())
}

async fn load_tree(user_id: Uuid) -> Result<TreeNode, String> {
    use gloo_net::http::Request;

    // Get root tree ID from context or storage
    let root_id = Uuid::new_v4(); // Replace with actual logic

    let response = Request::get(&format!("/api/agent/tree/{}/{}", user_id, root_id))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.json::<TreeNode>().await.map_err(|e| e.to_string())
}

// ============================================================================
// CSS Styles
// ============================================================================

#[component]
pub fn ChatStyles() -> impl IntoView {
    view! {
        <style>
            r#"
            .chat-container {
                width: 100%;
                height: 100vh;
                overflow: hidden;
            }

            .chat-layout {
                display: grid;
                grid-template-columns: 300px 1fr;
                height: 100%;
                gap: 1rem;
            }

            .tree-panel {
                border-right: 1px solid #e0e0e0;
                padding: 1rem;
                overflow-y: auto;
                background: #f9f9f9;
            }

            .tree-node {
                margin-left: 1rem;
            }

            .node-header {
                padding: 0.5rem;
                margin: 0.25rem 0;
                border-radius: 4px;
                cursor: pointer;
                transition: background 0.2s;
            }

            .node-header:hover {
                background: #e8e8e8;
            }

            .node-header.selected {
                background: #d0e8ff;
                border: 2px solid #1976d2;
            }

            .node-content {
                display: flex;
                align-items: center;
                gap: 0.5rem;
            }

            .image-node img {
                width: 60px;
                height: 60px;
                object-fit: cover;
                border-radius: 4px;
            }

            .chat-panel {
                display: flex;
                flex-direction: column;
                height: 100%;
            }

            .messages-container {
                flex: 1;
                overflow-y: auto;
                padding: 1rem;
                display: flex;
                flex-direction: column;
                gap: 1rem;
            }

            .message {
                padding: 1rem;
                border-radius: 8px;
                max-width: 70%;
            }

            .message.user {
                background: #1976d2;
                color: white;
                align-self: flex-end;
            }

            .message.assistant {
                background: #f0f0f0;
                color: #333;
                align-self: flex-start;
            }

            .message.streaming {
                opacity: 0.8;
                animation: pulse 1.5s infinite;
            }

            @keyframes pulse {
                0%, 100% { opacity: 0.8; }
                50% { opacity: 1; }
            }

            .input-container {
                border-top: 1px solid #e0e0e0;
                padding: 1rem;
                display: flex;
                flex-direction: column;
                gap: 0.5rem;
            }

            .selected-nodes-indicator {
                background: #e3f2fd;
                padding: 0.5rem;
                border-radius: 4px;
                display: flex;
                justify-content: space-between;
                align-items: center;
            }

            textarea {
                padding: 0.75rem;
                border: 1px solid #ccc;
                border-radius: 4px;
                resize: vertical;
                min-height: 60px;
                font-family: inherit;
            }

            button {
                padding: 0.75rem 1.5rem;
                background: #1976d2;
                color: white;
                border: none;
                border-radius: 4px;
                cursor: pointer;
                font-weight: 500;
            }

            button:hover:not(:disabled) {
                background: #1565c0;
            }

            button:disabled {
                background: #ccc;
                cursor: not-allowed;
            }
            "#
        </style>
    }
}
