//! AI Assistant Integration
//!
//! Provides AI-powered command assistance with support for multiple providers:
//! - Gemini (Google AI)
//! - Ollama (Local LLM)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// AI message for chat history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

/// AI configuration stored securely
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiConfig {
    pub gemini_api_key: Option<String>,
    pub grok_api_key: Option<String>,
    pub ollama_host: String,
    pub ollama_model: String,
}

impl AiConfig {
    pub fn default_config() -> Self {
        Self {
            gemini_api_key: None,
            grok_api_key: None,
            ollama_host: "http://localhost:11434".to_string(),
            ollama_model: "llama3.2".to_string(),
        }
    }
}

/// System prompt for network engineering assistance
const SYSTEM_PROMPT: &str = r#"You are a network engineering assistant integrated into an SSH terminal client.
You help with CLI commands for network devices including:
- Cisco IOS, IOS-XE, NX-OS
- Juniper JunOS
- Arista EOS
- Linux/Unix systems
- HP/Aruba switches
- Palo Alto firewalls
- And other network equipment

When suggesting commands:
1. Be specific to the device type when known
2. Explain what commands do briefly
3. Warn about potentially disruptive commands
4. Offer alternatives when available
5. Format commands in code blocks

The user may share terminal output for context. Analyze it to understand the device type and current configuration mode."#;

/// Get the AI config file path
fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("spurx-secure-ssh");
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("ai_config.json")
}

/// Load AI configuration from secure file
pub fn load_config() -> AiConfig {
    let path = get_config_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
    }
    AiConfig::default_config()
}

/// Save AI configuration to secure file
pub fn save_config(config: &AiConfig) -> Result<(), String> {
    let path = get_config_path();
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {}", e))?;
    Ok(())
}

/// Chat with Gemini API
pub async fn chat_gemini(api_key: &str, messages: &[AiMessage]) -> Result<String, String> {
    let client = reqwest::Client::new();

    // Build contents array with system prompt and messages
    let mut contents: Vec<serde_json::Value> = vec![];

    // Add system instruction as first user message context
    contents.push(serde_json::json!({
        "role": "user",
        "parts": [{"text": format!("System instructions: {}\n\nI'll now ask you questions.", SYSTEM_PROMPT)}]
    }));
    contents.push(serde_json::json!({
        "role": "model",
        "parts": [{"text": "Understood. I'm ready to help with network CLI commands and terminal analysis. What would you like help with?"}]
    }));

    // Add conversation history
    for msg in messages {
        let role = if msg.role == "user" { "user" } else { "model" };
        contents.push(serde_json::json!({
            "role": role,
            "parts": [{"text": &msg.content}]
        }));
    }

    let request_body = serde_json::json!({
        "contents": contents,
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 2048,
        }
    });

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Gemini API error: {}", error_text));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Extract text from response
    response_json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No response text found".to_string())
}

/// Chat with Ollama (local LLM)
pub async fn chat_ollama(
    host: &str,
    model: &str,
    messages: &[AiMessage],
) -> Result<String, String> {
    let client = reqwest::Client::new();

    // Build messages array with system prompt
    let mut ollama_messages: Vec<serde_json::Value> = vec![serde_json::json!({
        "role": "system",
        "content": SYSTEM_PROMPT
    })];

    // Add conversation history
    for msg in messages {
        ollama_messages.push(serde_json::json!({
            "role": &msg.role,
            "content": &msg.content
        }));
    }

    let request_body = serde_json::json!({
        "model": model,
        "messages": ollama_messages,
        "stream": false
    });

    let url = format!("{}/api/chat", host);

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Ollama connection failed: {}. Is Ollama running?", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Ollama API error: {}", error_text));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

    // Extract message content
    response_json["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No response from Ollama".to_string())
}

/// Chat with Grok (xAI) API
pub async fn chat_grok(api_key: &str, messages: &[AiMessage]) -> Result<String, String> {
    let client = reqwest::Client::new();

    // Build messages array with system prompt (OpenAI-compatible format)
    let mut grok_messages: Vec<serde_json::Value> = vec![serde_json::json!({
        "role": "system",
        "content": SYSTEM_PROMPT
    })];

    // Add conversation history
    for msg in messages {
        grok_messages.push(serde_json::json!({
            "role": &msg.role,
            "content": &msg.content
        }));
    }

    let request_body = serde_json::json!({
        "model": "grok-2-latest",
        "messages": grok_messages,
        "temperature": 0.7,
        "max_tokens": 2048
    });

    let response = client
        .post("https://api.x.ai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Grok request failed: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Grok API error: {}", error_text));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Grok response: {}", e))?;

    // Extract message content (OpenAI format)
    response_json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No response from Grok".to_string())
}

/// Main chat function that dispatches to the appropriate provider
pub async fn chat(provider: &str, messages: Vec<AiMessage>) -> Result<String, String> {
    let config = load_config();

    match provider {
        "gemini" => {
            let api_key = config.gemini_api_key.as_ref().ok_or_else(|| {
                "Gemini API key not configured. Click the settings icon to add your API key."
                    .to_string()
            })?;
            chat_gemini(api_key, &messages).await
        }
        "grok" => {
            let api_key = config.grok_api_key.as_ref().ok_or_else(|| {
                "Grok API key not configured. Click the settings icon to add your API key."
                    .to_string()
            })?;
            chat_grok(api_key, &messages).await
        }
        "ollama" => chat_ollama(&config.ollama_host, &config.ollama_model, &messages).await,
        _ => Err(format!("Unknown AI provider: {}", provider)),
    }
}
