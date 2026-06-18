# Model Settings API

This document is for the `gmr` UI integration with `cx58-agent`.

## Summary

The agent now stores model settings per user. Environment variables are startup
defaults only:

```text
VISION_MODEL=qwen3-vl:8b
TEXT_MODEL=qwen3.5:9b
CHAT_MODEL=qwen3.5:9b
```

When a user has no saved settings, the API returns these defaults as the current
settings. After the UI updates models, the agent stores them in PostgreSQL in
`user_model_settings`.

All model settings routes are HMAC-protected with the same signing contract as
`POST /agent/chat`.

## Capabilities

The agent reads Ollama model capabilities from:

- `GET /api/tags`
- `POST /api/show`

Role mapping:

| Agent role | Required Ollama capability |
| ---------- | -------------------------- |
| `vision_model` | `vision` |
| `text_model` | `completion` |
| `chat_model` | `tools` |

`VISION_MODEL` is the main model. If UI changes it, the agent first checks that
the model supports `vision`. With `same=true`, the agent then tries to use the
same model for `text_model` and `chat_model`; unsupported roles are left
unchanged and reported in `changes`.

## List Models And Current Settings

```http
GET /agent/models/{user_id}
```

Optional capability filter:

```http
GET /agent/models/{user_id}?capability=vision
GET /agent/models/{user_id}?capability=text
GET /agent/models/{user_id}?capability=tools
```

Accepted aliases:

| UI query | Normalized capability |
| -------- | --------------------- |
| `vision`, `visual`, `image` | `vision` |
| `text`, `completion`, `chat` | `text` |
| `tools`, `tool` | `tools` |

Response:

```json
{
  "user_id": "user-123",
  "current": {
    "user_id": "user-123",
    "vision_model": "qwen3-vl:8b",
    "text_model": "qwen3.5:9b",
    "chat_model": "qwen3.5:9b"
  },
  "defaults": {
    "user_id": "user-123",
    "vision_model": "qwen3-vl:8b",
    "text_model": "qwen3.5:9b",
    "chat_model": "qwen3.5:9b"
  },
  "models": [
    {
      "name": "qwen3-vl:8b",
      "size": 6100000000,
      "modified_at": "2026-06-18T12:00:00Z",
      "capabilities": ["completion", "vision"],
      "family": "qwen3vl",
      "parameter_size": "8B",
      "quantization_level": "Q4_K_M"
    }
  ],
  "capability": "vision"
}
```

When no filter is used, `capability` is omitted or `null` and `models` contains
all locally installed Ollama models that the agent can inspect.

## Update Models

```http
PUT /agent/models/{user_id}
Content-Type: application/json
```

Set only vision model:

```json
{
  "vision_model": "qwen3-vl:8b",
  "same": false
}
```

Set vision model and try to reuse it for text/tools:

```json
{
  "vision_model": "qwen3-vl:8b",
  "same": true
}
```

Set roles explicitly:

```json
{
  "vision_model": "qwen3-vl:8b",
  "text_model": "qwen3.5:9b",
  "chat_model": "qwen3.5:9b",
  "same": false
}
```

All fields are optional, but an empty request only persists the existing or
default settings.

Response:

```json
{
  "user_id": "user-123",
  "current": {
    "user_id": "user-123",
    "vision_model": "qwen3-vl:8b",
    "text_model": "qwen3.5:9b",
    "chat_model": "qwen3.5:9b"
  },
  "changes": [
    {
      "role": "vision_model",
      "model": "qwen3-vl:8b",
      "applied": true,
      "reason": "Applied"
    },
    {
      "role": "chat_model",
      "model": "qwen3-vl:8b",
      "applied": false,
      "reason": "Model does not support required capability: tools"
    }
  ]
}
```

If a requested model does not exist in Ollama, the route returns a `400` error.
If Ollama is unreachable, the route returns a service error.

## HMAC Signing Reminder

Headers:

```text
X-Timestamp: <unix timestamp seconds>
X-Signature: <hex HMAC-SHA256>
```

Signature payload:

```text
HMAC-SHA256(AGENT_SECRET, timestamp_bytes || body_bytes)
```

For `GET`, sign the empty body unless the UI proxy already has a shared helper
that signs requests consistently.

## Runtime Behavior

Every `/agent/chat` request resolves the effective user model settings before
processing. The rest of the request uses those models for:

- intent classification: `chat_model`
- orchestration and formatting: `text_model`
- RAG fallback chat: `text_model`
- image description: `vision_model`
- report comparison: `text_model`

