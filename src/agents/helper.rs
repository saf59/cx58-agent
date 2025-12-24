use rig::client::Nothing;
use rig::providers::ollama;
use rig::providers::ollama::Client;
// visual
// tool
// 0..1 thinking: Qwen, DeepSeek
pub const LOCAL_MODELS: &[&str] = &[
    "qwen3-vl:235b-cloud",
    "deepseek-v3.1:671b-cloud",
    "deepseek-r1",
    // small models: < 7.8G
    "llava",
    "llama3.2-vision",
    "functiongemma",
];
/* removed !!!
"zeffmuks/universal-ner:latest",
 */
// 0..2 thinking: Qwen, DeepSeek
pub const REMOTE_MODELS: &[&str] = &[
    "qwen3:14b",
    "qwen3-vl",
    "deepseek-r1:14b",
    "ministral-3:14b",
    "gemma3:12b",
    "minicpm-v:8b",
    "llava",
    "llama3.2-vision",
    "llava-llama3:latest",
    "functiongemma",
];
/* removed !!!
"mistral-nemo:12b",
"iodose/nuextract-v1.5:latest",
"ALIENTELLIGENCE/structureddataextraction:latest",
 */

pub fn client(is_local: bool) -> ollama::Client {
    if is_local {
        ollama::Client::new(Nothing).unwrap()
    } else {
        ollama::Client::builder()
            .api_key(Nothing)
            .base_url("http://localhost:8050")
            .build()
            .unwrap()
    }
}
pub fn check_model(model: &str, is_local: bool) -> bool {
    if is_local {
        LOCAL_MODELS.contains(&model)
    } else {
        REMOTE_MODELS.contains(&model)
    }
}
