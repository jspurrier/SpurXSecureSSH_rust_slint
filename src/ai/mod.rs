use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: Option<String>,
    pub selected_model: Option<String>,
    pub gemini_api_key: Option<String>,
    pub gemini_model: Option<String>,
    pub grok_api_key: Option<String>,
    pub grok_model: Option<String>,
    pub ollama_host: String,
    pub ollama_model: String,
    pub lmstudio_host: String,
    pub lmstudio_model: String,
    pub lmstudio_api_key: Option<String>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

impl AiConfig {
    pub fn default_config() -> Self {
        Self {
            provider: Some("ollama".to_string()),
            selected_model: Some("llama3.2".to_string()),
            gemini_api_key: None,
            gemini_model: Some("gemini-3.6-flash".to_string()),
            grok_api_key: None,
            grok_model: Some("grok-4.5".to_string()),
            ollama_host: "http://localhost:11434".to_string(),
            ollama_model: "llama3.2".to_string(),
            lmstudio_host: "http://localhost:1234/v1".to_string(),
            lmstudio_model: "local-model".to_string(),
            lmstudio_api_key: None,
        }
    }
}

const SYSTEM_PROMPT: &str = r#"You are a network engineering assistant integrated into an SSH terminal client.
You help with CLI commands for network devices including:
- Cisco IOS, IOS-XE, IOS-XR, NX-OS
- Juniper JunOS
- Nokia SR OS
- Arista EOS
- Linux/Unix systems
- MikroTik RouterOS
- Fortinet FortiOS & Palo Alto firewalls

When suggesting commands:
1. Be specific to the device type when known
2. Explain what commands do briefly
3. Warn about potentially disruptive commands
4. Offer alternatives when available
5. Format commands in code blocks

The user may share terminal output for context. Analyze it to understand the device type and current configuration mode."#;

fn get_config_path() -> PathBuf {
    let config_dir = crate::get_app_config_dir();
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("ai_config.json")
}

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

pub fn save_config(config: &AiConfig) -> Result<(), String> {
    let path = get_config_path();
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {}", e))?;
    Ok(())
}

pub async fn chat_gemini(
    api_key: &str,
    model: &str,
    messages: &[AiMessage],
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut contents: Vec<serde_json::Value> = vec![];

    contents.push(serde_json::json!({
        "role": "user",
        "parts": [{"text": format!("System instructions: {}\n\nI'll now ask you questions.", SYSTEM_PROMPT)}]
    }));
    contents.push(serde_json::json!({
        "role": "model",
        "parts": [{"text": "Understood. I'm ready to help with network CLI commands and terminal analysis. What would you like help with?"}]
    }));

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

    let model_name = match model.trim() {
        "" | "gemini-2.0-flash" | "gemini-2.0" | "gemini-2.5-flash" | "gemini-2.5" | "gemini-1.5-flash" | "gemini-1.5-pro" => "gemini-3.6-flash",
        other => other,
    };
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model_name, api_key
    );

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        let msg = if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(&error_text) {
            err_json["error"]["message"]
                .as_str()
                .unwrap_or(&error_text)
                .to_string()
        } else {
            error_text
        };
        return Err(format!("Gemini API Error ({}): {}", status, msg));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    response_json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No response text found in Gemini response".to_string())
}

pub async fn chat_ollama(
    host: &str,
    model: &str,
    messages: &[AiMessage],
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut ollama_messages: Vec<serde_json::Value> = vec![serde_json::json!({
        "role": "system",
        "content": SYSTEM_PROMPT
    })];

    for msg in messages {
        ollama_messages.push(serde_json::json!({
            "role": &msg.role,
            "content": &msg.content
        }));
    }

    let model_name = if model.is_empty() { "llama3.2" } else { model };
    let request_body = serde_json::json!({
        "model": model_name,
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

    response_json["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No response from Ollama".to_string())
}

pub async fn chat_grok(
    api_key: &str,
    model: &str,
    messages: &[AiMessage],
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut grok_messages: Vec<serde_json::Value> = vec![serde_json::json!({
        "role": "system",
        "content": SYSTEM_PROMPT
    })];

    for msg in messages {
        grok_messages.push(serde_json::json!({
            "role": &msg.role,
            "content": &msg.content
        }));
    }

    let model_name = if model.is_empty() { "grok-4.5" } else { model };
    let request_body = serde_json::json!({
        "model": model_name,
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

    response_json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No response from Grok".to_string())
}

pub async fn chat_lmstudio(
    host: &str,
    model: &str,
    api_key: Option<&str>,
    messages: &[AiMessage],
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut openai_messages: Vec<serde_json::Value> = vec![serde_json::json!({
        "role": "system",
        "content": SYSTEM_PROMPT
    })];

    for msg in messages {
        openai_messages.push(serde_json::json!({
            "role": &msg.role,
            "content": &msg.content
        }));
    }

    let model_name = if model.trim().is_empty() { "local-model" } else { model.trim() };
    let request_body = serde_json::json!({
        "model": model_name,
        "messages": openai_messages,
        "temperature": 0.7,
        "max_tokens": 2048
    });

    let base = host.trim_end_matches('/');
    let url = if base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{}/chat/completions", base)
    } else {
        format!("{}/v1/chat/completions", base)
    };

    let mut req = client.post(&url).header("Content-Type", "application/json");
    if let Some(key) = api_key {
        if !key.trim().is_empty() {
            req = req.header("Authorization", format!("Bearer {}", key.trim()));
        }
    }

    let response = req
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("LM Studio connection failed: {}. Is LM Studio server running?", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("LM Studio API error: {}", error_text));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse LM Studio response: {}", e))?;

    response_json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No response from LM Studio".to_string())
}

pub async fn chat_with_model(
    provider: &str,
    model: &str,
    messages: Vec<AiMessage>,
) -> Result<String, String> {
    let config = load_config();

    match provider {
        "gemini" => {
            let api_key = config.gemini_api_key.as_ref().ok_or_else(|| {
                "Gemini API key not configured. Click the settings ⚙ icon to add your API key."
                    .to_string()
            })?;
            let mod_name = if !model.is_empty() {
                model
            } else if let Some(ref m) = config.gemini_model {
                m.as_str()
            } else {
                "gemini-3.6-flash"
            };
            chat_gemini(api_key, mod_name, &messages).await
        }
        "grok" => {
            let api_key = config.grok_api_key.as_ref().ok_or_else(|| {
                "Grok API key not configured. Click the settings ⚙ icon to add your API key."
                    .to_string()
            })?;
            let mod_name = if !model.is_empty() {
                model
            } else if let Some(ref m) = config.grok_model {
                m.as_str()
            } else {
                "grok-4.5"
            };
            chat_grok(api_key, mod_name, &messages).await
        }
        "ollama" => {
            let mod_name = if !model.is_empty() {
                model
            } else if !config.ollama_model.is_empty() {
                &config.ollama_model
            } else {
                "llama3.2"
            };
            chat_ollama(&config.ollama_host, mod_name, &messages).await
        }
        "lmstudio" => {
            let mod_name = if !model.is_empty() {
                model
            } else if !config.lmstudio_model.is_empty() {
                &config.lmstudio_model
            } else {
                "local-model"
            };
            chat_lmstudio(
                &config.lmstudio_host,
                mod_name,
                config.lmstudio_api_key.as_deref(),
                &messages,
            )
            .await
        }
        _ => Err(format!("Unknown AI provider: {}", provider)),
    }
}

pub async fn chat(provider: &str, messages: Vec<AiMessage>) -> Result<String, String> {
    chat_with_model(provider, "", messages).await
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiAutocompleteSuggestion {
    pub cmd: String,
    pub desc: String,
}

fn strip_ansi_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn extract_typed_command(buffer_text: &str) -> String {
    let clean = strip_ansi_codes(buffer_text);
    if let Some(last_line) = clean.lines().rev().find(|l| !l.trim().is_empty()) {
        let trimmed = last_line.trim_end();
        let delimiters = [
            "# ", "> ", "$ ", "% ", ":~$ ", "]: ", "#", ">", "$", "%",
        ];
        for delim in delimiters {
            if let Some(pos) = trimmed.rfind(delim) {
                let cmd_part = &trimmed[pos + delim.len()..];
                return cmd_part.trim().to_string();
            }
        }
        trimmed.trim().to_string()
    } else {
        String::new()
    }
}

pub fn get_catalog_for_platform(platform: &str) -> Vec<AiAutocompleteSuggestion> {
    match platform {
        "Cisco IOS-XR" => vec![
            AiAutocompleteSuggestion { cmd: "show ip interface brief".into(), desc: "List IP interface status and VRF summary".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces summary".into(), desc: "Aggregate operational interface statistics".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces description".into(), desc: "List interface port description labels".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces accounting".into(), desc: "Per-protocol packet counters on interfaces".into() },
            AiAutocompleteSuggestion { cmd: "show route".into(), desc: "Display IPv4/IPv6 RIB routing table".into() },
            AiAutocompleteSuggestion { cmd: "show route summary".into(), desc: "Overview of route counts per protocol".into() },
            AiAutocompleteSuggestion { cmd: "show route bgp".into(), desc: "Routes learned specifically from BGP".into() },
            AiAutocompleteSuggestion { cmd: "show route isis".into(), desc: "Routes learned via IS-IS protocol".into() },
            AiAutocompleteSuggestion { cmd: "show bgp summary".into(), desc: "BGP neighbor sessions and prefixes received".into() },
            AiAutocompleteSuggestion { cmd: "show bgp ipv4 unicast summary".into(), desc: "IPv4 Unicast address-family BGP session table".into() },
            AiAutocompleteSuggestion { cmd: "show bgp ipv6 unicast summary".into(), desc: "IPv6 Unicast address-family BGP session table".into() },
            AiAutocompleteSuggestion { cmd: "show bgp neighbors".into(), desc: "Detailed BGP neighbor configuration and stats".into() },
            AiAutocompleteSuggestion { cmd: "show isis neighbors".into(), desc: "IS-IS adjacencies, interface states, and hold times".into() },
            AiAutocompleteSuggestion { cmd: "show isis database".into(), desc: "IS-IS link state PDU database".into() },
            AiAutocompleteSuggestion { cmd: "show ospf neighbor".into(), desc: "OSPF neighbor status and area info".into() },
            AiAutocompleteSuggestion { cmd: "show mpls ldp neighbor".into(), desc: "MPLS LDP peer discovery and session state".into() },
            AiAutocompleteSuggestion { cmd: "show mpls forwarding".into(), desc: "MPLS label forwarding table (LFIB)".into() },
            AiAutocompleteSuggestion { cmd: "show running-config".into(), desc: "Display committed configuration".into() },
            AiAutocompleteSuggestion { cmd: "show configuration commit list".into(), desc: "History of configuration commits with timestamps".into() },
            AiAutocompleteSuggestion { cmd: "show configuration failed".into(), desc: "Show syntax errors in candidate configuration".into() },
            AiAutocompleteSuggestion { cmd: "commit".into(), desc: "Commit candidate configuration changes".into() },
            AiAutocompleteSuggestion { cmd: "commit replace".into(), desc: "Replace active configuration with candidate config".into() },
            AiAutocompleteSuggestion { cmd: "show platform".into(), desc: "Line card, RSP, and fabric node status".into() },
            AiAutocompleteSuggestion { cmd: "show system verify".into(), desc: "Integrity check of XR packages and running nodes".into() },
        ],
        "Cisco NX-OS" => vec![
            AiAutocompleteSuggestion { cmd: "show ip interface brief vrf all".into(), desc: "Show IP status across all VRFs".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces status".into(), desc: "Check switchport speeds, duplex, and VLANs".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces description".into(), desc: "List interface description labels".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces counters errors".into(), desc: "Check for CRC, collisions, and drop errors".into() },
            AiAutocompleteSuggestion { cmd: "show running-config".into(), desc: "Show active running switch configuration".into() },
            AiAutocompleteSuggestion { cmd: "show running-config | section".into(), desc: "Filter running config by section keyword".into() },
            AiAutocompleteSuggestion { cmd: "show ip route vrf all".into(), desc: "Display routing table across all VRFs".into() },
            AiAutocompleteSuggestion { cmd: "show vpc brief".into(), desc: "Display Virtual Port-Channel status and peer link".into() },
            AiAutocompleteSuggestion { cmd: "show bgp l2vpn evpn summary".into(), desc: "Check VXLAN EVPN BGP peer states".into() },
            AiAutocompleteSuggestion { cmd: "show nve peers".into(), desc: "VXLAN NVE VTEP tunnel peer status".into() },
            AiAutocompleteSuggestion { cmd: "show nve vni".into(), desc: "VXLAN VNI mapping and operational status".into() },
            AiAutocompleteSuggestion { cmd: "show port-channel summary".into(), desc: "Port-channel status and bundled member interfaces".into() },
            AiAutocompleteSuggestion { cmd: "show mac address-table".into(), desc: "L2 MAC forwarding table".into() },
            AiAutocompleteSuggestion { cmd: "show cdp neighbors".into(), desc: "List connected Cisco CDP neighbors".into() },
            AiAutocompleteSuggestion { cmd: "show lldp neighbors".into(), desc: "List connected LLDP IEEE 802.1AB neighbors".into() },
            AiAutocompleteSuggestion { cmd: "show system resources".into(), desc: "CPU load, memory allocation, and processes".into() },
            AiAutocompleteSuggestion { cmd: "show environment power".into(), desc: "Power supply modules and consumption".into() },
            AiAutocompleteSuggestion { cmd: "show version".into(), desc: "NX-OS release image, uptime, and switch model".into() },
        ],
        "Cisco IOS / IOS-XE" => vec![
            AiAutocompleteSuggestion { cmd: "show ip interface brief".into(), desc: "Summary of all IP interface states and addresses".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces status".into(), desc: "Switchport link states, speed, duplex, and VLANs".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces description".into(), desc: "List configured interface description labels".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces counters errors".into(), desc: "Check for CRC, collisions, and packet drop errors".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces switchport".into(), desc: "Detailed L2 switchport VLAN and trunking parameters".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces trunk".into(), desc: "List active 802.1Q trunk links and allowed VLANs".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces summary".into(), desc: "Aggregate interface count and traffic statistics".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces transceiver detail".into(), desc: "Optical SFP/QSFP DOM power levels and temperatures".into() },
            AiAutocompleteSuggestion { cmd: "show ip route".into(), desc: "Display complete IPv4 routing table".into() },
            AiAutocompleteSuggestion { cmd: "show ip route summary".into(), desc: "Count of routes per protocol (BGP, OSPF, Connected)".into() },
            AiAutocompleteSuggestion { cmd: "show ip bgp summary".into(), desc: "BGP neighbor sessions, prefixes received and uptime".into() },
            AiAutocompleteSuggestion { cmd: "show ip ospf neighbor".into(), desc: "OSPF neighbor adjacency states and Dead times".into() },
            AiAutocompleteSuggestion { cmd: "show ip ospf database".into(), desc: "OSPF Link-State Database overview".into() },
            AiAutocompleteSuggestion { cmd: "show ip nat translations".into(), desc: "Active NAT/PAT IP address translation table".into() },
            AiAutocompleteSuggestion { cmd: "show ip protocols".into(), desc: "Active routing protocols and configured networks".into() },
            AiAutocompleteSuggestion { cmd: "show ip arp".into(), desc: "Address Resolution Protocol table mapping IPs to MACs".into() },
            AiAutocompleteSuggestion { cmd: "show running-config".into(), desc: "Display complete active running device configuration".into() },
            AiAutocompleteSuggestion { cmd: "show running-config | section router".into(), desc: "Filter running config for routing protocol blocks".into() },
            AiAutocompleteSuggestion { cmd: "show running-config | include".into(), desc: "Search running config for specific keywords".into() },
            AiAutocompleteSuggestion { cmd: "show cdp neighbors".into(), desc: "List directly connected Cisco devices".into() },
            AiAutocompleteSuggestion { cmd: "show cdp neighbors detail".into(), desc: "Detailed neighbor IPs, software versions, and platforms".into() },
            AiAutocompleteSuggestion { cmd: "show lldp neighbors".into(), desc: "List connected LLDP IEEE 802.1AB neighbors".into() },
            AiAutocompleteSuggestion { cmd: "show vlan brief".into(), desc: "List all configured VLANs and assigned access ports".into() },
            AiAutocompleteSuggestion { cmd: "show mac address-table".into(), desc: "Layer 2 MAC forwarding table and port mappings".into() },
            AiAutocompleteSuggestion { cmd: "show mac address-table dynamic".into(), desc: "Dynamically learned MAC addresses".into() },
            AiAutocompleteSuggestion { cmd: "show spanning-tree brief".into(), desc: "STP topology, root bridges, and blocking ports".into() },
            AiAutocompleteSuggestion { cmd: "show etherchannel summary".into(), desc: "LACP/PAgP Port-Channel status and bundled ports".into() },
            AiAutocompleteSuggestion { cmd: "show processes cpu sorted".into(), desc: "Top CPU-consuming processes and 5s/1m/5m load".into() },
            AiAutocompleteSuggestion { cmd: "show processes memory sorted".into(), desc: "System memory consumption sorted by process".into() },
            AiAutocompleteSuggestion { cmd: "show logging | include".into(), desc: "Filter syslog buffer for errors, drops, or flaps".into() },
            AiAutocompleteSuggestion { cmd: "show version".into(), desc: "System uptime, software image, and serial numbers".into() },
            AiAutocompleteSuggestion { cmd: "show inventory".into(), desc: "Chassis serials, power supplies, and transceiver modules".into() },
            AiAutocompleteSuggestion { cmd: "show environment power".into(), desc: "Power supply status and PoE budget".into() },
            AiAutocompleteSuggestion { cmd: "configure terminal".into(), desc: "Enter global configuration mode".into() },
            AiAutocompleteSuggestion { cmd: "terminal length 0".into(), desc: "Disable terminal pagination for uninterrupted output".into() },
            AiAutocompleteSuggestion { cmd: "write memory".into(), desc: "Save running configuration to startup NVRAM".into() },
        ],
        "Juniper Junos OS" => vec![
            AiAutocompleteSuggestion { cmd: "show interfaces terse".into(), desc: "Compact interface operational list with IPs and status".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces descriptions".into(), desc: "All physical and logical interface description labels".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces extensive".into(), desc: "Deep packet statistics, errors, and optical DOM".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces statistics".into(), desc: "Detailed packet in/out rates and throughput".into() },
            AiAutocompleteSuggestion { cmd: "show route".into(), desc: "Display routing engine routing tables".into() },
            AiAutocompleteSuggestion { cmd: "show route summary".into(), desc: "Count of routes per protocol and RIB table".into() },
            AiAutocompleteSuggestion { cmd: "show route protocol bgp".into(), desc: "Routes learned specifically from BGP peers".into() },
            AiAutocompleteSuggestion { cmd: "show route protocol ospf".into(), desc: "Routes learned via OSPF protocol".into() },
            AiAutocompleteSuggestion { cmd: "show route forwarding-table".into(), desc: "Kernel forwarding engine FIB table".into() },
            AiAutocompleteSuggestion { cmd: "show bgp summary".into(), desc: "BGP peer session statuses, AS numbers, and rib counts".into() },
            AiAutocompleteSuggestion { cmd: "show bgp neighbor".into(), desc: "Deep BGP peer parameters and advertised/received routes".into() },
            AiAutocompleteSuggestion { cmd: "show ospf neighbor".into(), desc: "OSPF neighbor adjacencies and dead timers".into() },
            AiAutocompleteSuggestion { cmd: "show ospf interface".into(), desc: "OSPF enabled interfaces and area assignments".into() },
            AiAutocompleteSuggestion { cmd: "show lldp neighbors".into(), desc: "Connected LLDP neighbor chassis IDs and ports".into() },
            AiAutocompleteSuggestion { cmd: "show configuration | display set".into(), desc: "Display active config in set command format".into() },
            AiAutocompleteSuggestion { cmd: "show configuration compare rollback 1".into(), desc: "Diff current active config with previous rollback".into() },
            AiAutocompleteSuggestion { cmd: "show system uptime".into(), desc: "Current system uptime, system time, and load average".into() },
            AiAutocompleteSuggestion { cmd: "show system storage".into(), desc: "Disk space utilization across /var and /cf partitions".into() },
            AiAutocompleteSuggestion { cmd: "show system processes extensive".into(), desc: "Top running system processes and CPU usage".into() },
            AiAutocompleteSuggestion { cmd: "show chassis hardware".into(), desc: "Chassis serial, FPC, PIC, and transceiver inventory".into() },
            AiAutocompleteSuggestion { cmd: "show chassis routing-engine".into(), desc: "Routing engine mastership, CPU, and temperature".into() },
            AiAutocompleteSuggestion { cmd: "show chassis alarms".into(), desc: "Active hardware, chassis, and optical alarms".into() },
            AiAutocompleteSuggestion { cmd: "configure".into(), desc: "Enter configuration mode".into() },
            AiAutocompleteSuggestion { cmd: "commit check".into(), desc: "Validate syntax of configuration changes without activating".into() },
            AiAutocompleteSuggestion { cmd: "commit confirmed 5".into(), desc: "Commit with automatic rollback after 5 mins if unconfirmed".into() },
        ],
        "Nokia SR OS" => vec![
            AiAutocompleteSuggestion { cmd: "show router interface".into(), desc: "Display IP router network interfaces and operational states".into() },
            AiAutocompleteSuggestion { cmd: "show router route-table".into(), desc: "Display IP routing table".into() },
            AiAutocompleteSuggestion { cmd: "show router bgp summary".into(), desc: "BGP peer session summary and state".into() },
            AiAutocompleteSuggestion { cmd: "show router bgp neighbor".into(), desc: "Detailed BGP neighbor statistics and prefixes".into() },
            AiAutocompleteSuggestion { cmd: "show router isis neighbor".into(), desc: "IS-IS adjacency table".into() },
            AiAutocompleteSuggestion { cmd: "show router ospf neighbor".into(), desc: "OSPF neighbor state and interface status".into() },
            AiAutocompleteSuggestion { cmd: "show router arp".into(), desc: "Display ARP cache entries".into() },
            AiAutocompleteSuggestion { cmd: "show router status".into(), desc: "System router router-id and protocol operational status".into() },
            AiAutocompleteSuggestion { cmd: "show service service-using".into(), desc: "List all active VPLS, VPRN, Epipe, and IES services".into() },
            AiAutocompleteSuggestion { cmd: "show service id 1 base".into(), desc: "Summary of specific service instance parameters".into() },
            AiAutocompleteSuggestion { cmd: "show service sap-using".into(), desc: "List all configured Service Access Points (SAPs)".into() },
            AiAutocompleteSuggestion { cmd: "show service fdb-mac".into(), desc: "Forwarding Database MAC address table for VPLS".into() },
            AiAutocompleteSuggestion { cmd: "show port".into(), desc: "Display all physical port links, speeds, and MTUs".into() },
            AiAutocompleteSuggestion { cmd: "show port description".into(), desc: "List port description strings".into() },
            AiAutocompleteSuggestion { cmd: "show card".into(), desc: "List installed IOM, MDA, and CPM cards and states".into() },
            AiAutocompleteSuggestion { cmd: "show card state".into(), desc: "Hardware health and operational status of all cards".into() },
            AiAutocompleteSuggestion { cmd: "show system information".into(), desc: "Chassis type, software TiMOS release, and BOF location".into() },
            AiAutocompleteSuggestion { cmd: "show system memory".into(), desc: "CPM memory usage and free allocation pool".into() },
            AiAutocompleteSuggestion { cmd: "show system cpu".into(), desc: "CPM CPU utilization percentages".into() },
            AiAutocompleteSuggestion { cmd: "admin display-config".into(), desc: "Display complete router configuration text".into() },
            AiAutocompleteSuggestion { cmd: "admin save".into(), desc: "Save active configuration to primary storage CF/NVRAM".into() },
            AiAutocompleteSuggestion { cmd: "environment no-more".into(), desc: "Disable output paging for the session".into() },
        ],
        "Arista EOS" => vec![
            AiAutocompleteSuggestion { cmd: "show ip interface brief".into(), desc: "Display interface IP addresses and line status".into() },
            AiAutocompleteSuggestion { cmd: "show ip route".into(), desc: "Display IPv4 routing table".into() },
            AiAutocompleteSuggestion { cmd: "show ip route vrf all".into(), desc: "Display IPv4 routing table across all VRFs".into() },
            AiAutocompleteSuggestion { cmd: "show ip bgp summary".into(), desc: "BGP peer session summary and route counts".into() },
            AiAutocompleteSuggestion { cmd: "show ip ospf neighbor".into(), desc: "OSPF neighbor adjacencies".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces status".into(), desc: "Port link speeds, duplex, and VLAN membership".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces description".into(), desc: "Interface description labels".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces counters errors".into(), desc: "CRC and packet error counters on ports".into() },
            AiAutocompleteSuggestion { cmd: "show interfaces transceiver".into(), desc: "Optical transceiver diagnostics and laser levels".into() },
            AiAutocompleteSuggestion { cmd: "show running-config".into(), desc: "Display active EOS running configuration".into() },
            AiAutocompleteSuggestion { cmd: "show lldp neighbors".into(), desc: "List connected LLDP neighbors and system names".into() },
            AiAutocompleteSuggestion { cmd: "show vlan".into(), desc: "List active VLANs and assigned member interfaces".into() },
            AiAutocompleteSuggestion { cmd: "show mac address-table".into(), desc: "Layer 2 MAC forwarding table".into() },
            AiAutocompleteSuggestion { cmd: "show version".into(), desc: "EOS software release, model name, and uptime".into() },
            AiAutocompleteSuggestion { cmd: "show logging last 50".into(), desc: "Display last 50 syslog messages".into() },
        ],
        "MikroTik RouterOS" => vec![
            AiAutocompleteSuggestion { cmd: "/ip address print".into(), desc: "Display all configured IP addresses and interfaces".into() },
            AiAutocompleteSuggestion { cmd: "/ip route print".into(), desc: "Display complete IPv4 routing table".into() },
            AiAutocompleteSuggestion { cmd: "/ip firewall filter print".into(), desc: "List firewall security filter rules".into() },
            AiAutocompleteSuggestion { cmd: "/ip firewall nat print".into(), desc: "List NAT port forwarding and masquerade rules".into() },
            AiAutocompleteSuggestion { cmd: "/ip dhcp-server lease print".into(), desc: "Show active DHCP client IP leases".into() },
            AiAutocompleteSuggestion { cmd: "/ip dns print".into(), desc: "Display DNS cache and upstream server config".into() },
            AiAutocompleteSuggestion { cmd: "/interface print".into(), desc: "List all physical, VLAN, and bridge interfaces".into() },
            AiAutocompleteSuggestion { cmd: "/interface ethernet print".into(), desc: "Show Ethernet physical link parameters".into() },
            AiAutocompleteSuggestion { cmd: "/system resource print".into(), desc: "Display CPU load, free memory, and uptime".into() },
            AiAutocompleteSuggestion { cmd: "/log print follow".into(), desc: "Stream live RouterOS system event logs".into() },
        ],
        "Fortinet FortiOS" => vec![
            AiAutocompleteSuggestion { cmd: "get system status".into(), desc: "Display FortiOS version, firmware build, and serial".into() },
            AiAutocompleteSuggestion { cmd: "get system performance status".into(), desc: "CPU, memory, session count, and network throughput".into() },
            AiAutocompleteSuggestion { cmd: "get system interface physical".into(), desc: "List physical interface link speeds and duplex".into() },
            AiAutocompleteSuggestion { cmd: "get system session-info summary".into(), desc: "Total active firewall sessions and protocol counts".into() },
            AiAutocompleteSuggestion { cmd: "get router info routing-table all".into(), desc: "Display full IP routing table".into() },
            AiAutocompleteSuggestion { cmd: "get router info bgp summary".into(), desc: "Display BGP neighbor states and prefix counts".into() },
            AiAutocompleteSuggestion { cmd: "diagnose sys top".into(), desc: "Live interactive CPU and process monitor".into() },
            AiAutocompleteSuggestion { cmd: "diagnose hardware deviceinfo nic".into(), desc: "Inspect NIC hardware counters and link state".into() },
        ],
        "Adtran TA5000 OLT" => vec![
            AiAutocompleteSuggestion { cmd: "show shelf".into(), desc: "Display TA5000 shelf status, power feeds, fans, and slot provisioning".into() },
            AiAutocompleteSuggestion { cmd: "show card".into(), desc: "List installed OLT, SCM, and line cards across all slots".into() },
            AiAutocompleteSuggestion { cmd: "show card 1".into(), desc: "Detailed hardware status, firmware version, and alarms for slot".into() },
            AiAutocompleteSuggestion { cmd: "show gpon ont".into(), desc: "Display all discovered and provisioned ONTs/ONUs".into() },
            AiAutocompleteSuggestion { cmd: "show gpon ont status".into(), desc: "List optical Rx/Tx power levels, serial numbers, and distance".into() },
            AiAutocompleteSuggestion { cmd: "show gpon ont summary".into(), desc: "Summary counts of active, standby, and unprovisioned ONTs".into() },
            AiAutocompleteSuggestion { cmd: "show xgs-pon ont".into(), desc: "XGS-PON 10G symmetric ONT operational status and laser levels".into() },
            AiAutocompleteSuggestion { cmd: "show gpon profile".into(), desc: "List configured DBA, bandwidth profiles, and traffic descriptors".into() },
            AiAutocompleteSuggestion { cmd: "show interface pon".into(), desc: "Display PON OLT optical port states, laser status, and BER".into() },
            AiAutocompleteSuggestion { cmd: "show interface ethernet".into(), desc: "Uplink 10GE/100GE network interface status and optics".into() },
            AiAutocompleteSuggestion { cmd: "show vlan".into(), desc: "Display provisioned FTTP service VLANs and GEM port mappings".into() },
            AiAutocompleteSuggestion { cmd: "show alarms active".into(), desc: "Display current active critical, major, and minor chassis alarms".into() },
            AiAutocompleteSuggestion { cmd: "show alarms history".into(), desc: "View historical alarm log and clear events".into() },
            AiAutocompleteSuggestion { cmd: "show running-config".into(), desc: "Display active TA5000 running configuration".into() },
            AiAutocompleteSuggestion { cmd: "show version".into(), desc: "System software release, SCM firmware, and boot code".into() },
            AiAutocompleteSuggestion { cmd: "show fiber-stats".into(), desc: "Optical link budget, attenuation, and reflectometry stats".into() },
            AiAutocompleteSuggestion { cmd: "show mac-address-table".into(), desc: "Bridge forwarding table and MAC addresses per GEM port".into() },
            AiAutocompleteSuggestion { cmd: "provision ont".into(), desc: "Provision new ONT with serial number, model, and profile".into() },
        ],
        "Adtran SDX Switch" => vec![
            AiAutocompleteSuggestion { cmd: "show interface status".into(), desc: "Display port link status, speed, duplex, and transceiver DOM".into() },
            AiAutocompleteSuggestion { cmd: "show interface transceiver detail".into(), desc: "Optical DOM laser Tx/Rx power and temperatures".into() },
            AiAutocompleteSuggestion { cmd: "show vlan summary".into(), desc: "Summary of configured VLANs and trunk ports".into() },
            AiAutocompleteSuggestion { cmd: "show port-channel summary".into(), desc: "LACP link aggregation groups and member ports".into() },
            AiAutocompleteSuggestion { cmd: "show ip interface brief".into(), desc: "Management and in-band IP interface overview".into() },
            AiAutocompleteSuggestion { cmd: "show lldp neighbors".into(), desc: "Connected neighboring switches, routers, and OLTs".into() },
            AiAutocompleteSuggestion { cmd: "show mac address-table".into(), desc: "Layer 2 MAC forwarding table and EVPN bindings".into() },
            AiAutocompleteSuggestion { cmd: "show running-config".into(), desc: "Display committed SDX switch configuration".into() },
            AiAutocompleteSuggestion { cmd: "show system info".into(), desc: "System model, SDX OS version, CPU, and memory".into() },
            AiAutocompleteSuggestion { cmd: "show environment power".into(), desc: "Power supply module states and thermal sensor readings".into() },
            AiAutocompleteSuggestion { cmd: "show logs".into(), desc: "Display recent switch event logs and alarms".into() },
        ],
        _ => vec![
            AiAutocompleteSuggestion { cmd: "systemctl status".into(), desc: "Check status and recent logs of systemd services".into() },
            AiAutocompleteSuggestion { cmd: "systemctl restart".into(), desc: "Restart a systemd service".into() },
            AiAutocompleteSuggestion { cmd: "systemctl stop".into(), desc: "Stop a running systemd service".into() },
            AiAutocompleteSuggestion { cmd: "systemctl start".into(), desc: "Start a stopped systemd service".into() },
            AiAutocompleteSuggestion { cmd: "systemctl enable --now".into(), desc: "Enable and immediately start a service on boot".into() },
            AiAutocompleteSuggestion { cmd: "systemctl daemon-reload".into(), desc: "Reload systemd manager configuration after unit edit".into() },
            AiAutocompleteSuggestion { cmd: "systemctl list-units --failed".into(), desc: "List all failed systemd services and units".into() },
            AiAutocompleteSuggestion { cmd: "journalctl -xe --no-pager".into(), desc: "View recent system logs with full error diagnostics".into() },
            AiAutocompleteSuggestion { cmd: "journalctl -u ... -f".into(), desc: "Follow live logs of a specific systemd unit".into() },
            AiAutocompleteSuggestion { cmd: "ip a".into(), desc: "Display all network interfaces, MACs, and assigned IP addresses".into() },
            AiAutocompleteSuggestion { cmd: "ip route".into(), desc: "Display Linux kernel routing table and default gateway".into() },
            AiAutocompleteSuggestion { cmd: "ip link show".into(), desc: "List physical and virtual network link states and MTUs".into() },
            AiAutocompleteSuggestion { cmd: "ip neigh show".into(), desc: "Display ARP neighbor cache table".into() },
            AiAutocompleteSuggestion { cmd: "ip -br a".into(), desc: "Compact brief interface address table".into() },
            AiAutocompleteSuggestion { cmd: "docker ps -a".into(), desc: "List all active and stopped Docker containers".into() },
            AiAutocompleteSuggestion { cmd: "docker logs -f".into(), desc: "Stream logs of a specific Docker container".into() },
            AiAutocompleteSuggestion { cmd: "docker compose up -d".into(), desc: "Start multi-container app defined in docker-compose.yml".into() },
            AiAutocompleteSuggestion { cmd: "docker compose down".into(), desc: "Stop and remove containers, networks, and volumes".into() },
            AiAutocompleteSuggestion { cmd: "docker stats".into(), desc: "Live CPU, memory, and network I/O usage of containers".into() },
            AiAutocompleteSuggestion { cmd: "docker image ls".into(), desc: "List locally cached Docker container images".into() },
            AiAutocompleteSuggestion { cmd: "docker system df".into(), desc: "Display Docker disk space usage for containers and volumes".into() },
            AiAutocompleteSuggestion { cmd: "ss -tulpn".into(), desc: "List all listening TCP and UDP ports and process names".into() },
            AiAutocompleteSuggestion { cmd: "df -h".into(), desc: "Show disk filesystem capacity and free space in GB/MB".into() },
            AiAutocompleteSuggestion { cmd: "du -sh * | sort -h".into(), desc: "Calculate directory disk usage sorted by size".into() },
            AiAutocompleteSuggestion { cmd: "lsblk".into(), desc: "List block storage devices, disks, and partition mount points".into() },
            AiAutocompleteSuggestion { cmd: "free -m".into(), desc: "Display RAM memory and swap utilization in megabytes".into() },
            AiAutocompleteSuggestion { cmd: "top".into(), desc: "Display real-time Linux processes and CPU/RAM load".into() },
            AiAutocompleteSuggestion { cmd: "ps aux | grep".into(), desc: "Search active process table for specific binary name".into() },
            AiAutocompleteSuggestion { cmd: "git status".into(), desc: "Show current Git working directory changes and untracked files".into() },
            AiAutocompleteSuggestion { cmd: "git diff".into(), desc: "Display unstaged code diffs".into() },
            AiAutocompleteSuggestion { cmd: "git log --oneline -n 20".into(), desc: "View last 20 git commits in concise single-line format".into() },
            AiAutocompleteSuggestion { cmd: "git pull".into(), desc: "Fetch and merge remote changes into current branch".into() },
            AiAutocompleteSuggestion { cmd: "curl -Iv".into(), desc: "Inspect HTTP response headers, SSL handshake, and status codes".into() },
            AiAutocompleteSuggestion { cmd: "ping -c 4".into(), desc: "Send 4 ICMP echo packets to test network connectivity".into() },
            AiAutocompleteSuggestion { cmd: "nc -zv".into(), desc: "Test TCP socket connectivity to remote host and port".into() },
        ],
    }
}

// Learned custom commands store per platform (~/.config/spurx-secure-ssh/learned_commands.json)
fn get_learned_commands_file() -> PathBuf {
    let config_dir = crate::get_app_config_dir();
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("learned_commands.json")
}

lazy_static::lazy_static! {
    static ref LEARNED_COMMANDS_CACHE: parking_lot::RwLock<HashMap<String, Vec<AiAutocompleteSuggestion>>> = {
        let file = get_learned_commands_file();
        let mut map = HashMap::new();
        if file.exists() {
            if let Ok(data) = std::fs::read_to_string(&file) {
                if let Ok(m) = serde_json::from_str::<HashMap<String, Vec<AiAutocompleteSuggestion>>>(&data) {
                    map = m;
                }
            }
        }
        parking_lot::RwLock::new(map)
    };
}

pub fn load_learned_commands() -> HashMap<String, Vec<AiAutocompleteSuggestion>> {
    LEARNED_COMMANDS_CACHE.read().clone()
}

pub fn save_learned_command(platform: &str, cmd: &str, desc: &str) {
    let cmd_trimmed = cmd.trim().to_string();
    if cmd_trimmed.is_empty() || cmd_trimmed.len() > 120 {
        return;
    }
    let mut cache = LEARNED_COMMANDS_CACHE.write();
    let list = cache.entry(platform.to_string()).or_default();
    if !list.iter().any(|item| item.cmd == cmd_trimmed) {
        list.insert(
            0,
            AiAutocompleteSuggestion {
                cmd: cmd_trimmed,
                desc: if desc.trim().is_empty() {
                    "Custom learned command".to_string()
                } else {
                    desc.trim().to_string()
                },
            },
        );
        // Keep max 50 learned commands per platform
        list.truncate(50);
        let file = get_learned_commands_file();
        if let Ok(json) = serde_json::to_string_pretty(&*cache) {
            let _ = std::fs::write(file, json);
        }
    }
}

pub fn filter_suggestions_by_prefix(
    catalog: &[AiAutocompleteSuggestion],
    typed_prefix: &str,
) -> Vec<AiAutocompleteSuggestion> {
    let clean_prefix = typed_prefix.trim().to_lowercase();
    if clean_prefix.is_empty() {
        return catalog.to_vec();
    }

    let prefix_words: Vec<&str> = clean_prefix.split_whitespace().collect();
    let mut exact_matches = Vec::new();
    let mut word_matches = Vec::new();
    let mut desc_matches = Vec::new();

    for item in catalog {
        let cmd_lower = item.cmd.to_lowercase();
        let desc_lower = item.desc.to_lowercase();

        if cmd_lower.starts_with(&clean_prefix) {
            exact_matches.push(item.clone());
        } else if prefix_words.iter().all(|&w| cmd_lower.contains(w)) {
            word_matches.push(item.clone());
        } else if prefix_words.iter().all(|&w| desc_lower.contains(w)) {
            desc_matches.push(item.clone());
        }
    }

    let mut result = exact_matches;
    for m in word_matches {
        if !result.iter().any(|r| r.cmd == m.cmd) {
            result.push(m);
        }
    }
    for m in desc_matches {
        if !result.iter().any(|r| r.cmd == m.cmd) {
            result.push(m);
        }
    }

    if result.is_empty() {
        catalog.to_vec()
    } else {
        result
    }
}

pub fn detect_platform_from_terminal(buffer_text: &str) -> (String, Vec<AiAutocompleteSuggestion>) {
    let (platform, _, suggestions) = detect_platform_and_suggestions(buffer_text);
    (platform, suggestions)
}

pub fn detect_platform_and_suggestions(buffer_text: &str) -> (String, String, Vec<AiAutocompleteSuggestion>) {
    let lower = buffer_text.to_lowercase();
    let typed_cmd = extract_typed_command(buffer_text);

    let platform = if lower.contains("ta5000")
        || lower.contains("total access 5000")
        || lower.contains("adtran-ta5000")
        || (lower.contains("adtran") && (lower.contains("olt") || lower.contains("gpon") || lower.contains("xgs") || lower.contains("shelf") || lower.contains("scm")))
    {
        "Adtran TA5000 OLT"
    } else if (lower.contains("adtran") && (lower.contains("sdx") || lower.contains("switch")))
        || lower.contains("sdx-")
        || lower.contains("sdx6")
        || lower.contains("sdx8")
    {
        "Adtran SDX Switch"
    } else if lower.contains("ios xr")
        || lower.contains("ios-xr")
        || lower.contains("rp/0/")
        || lower.contains("sysadmin#")
    {
        "Cisco IOS-XR"
    } else if lower.contains("nexus") || lower.contains("nx-os") || lower.contains("nxos") {
        "Cisco NX-OS"
    } else if lower.contains("cisco")
        || lower.contains("ios-xe")
        || lower.contains("catalyst")
        || lower.contains("switch#")
        || lower.contains("router#")
        || lower.contains("(config)#")
    {
        "Cisco IOS / IOS-XE"
    } else if lower.contains("junos")
        || lower.contains("juniper")
        || lower.contains(">{master}")
        || (lower.contains("user@") && lower.contains(">"))
    {
        "Juniper Junos OS"
    } else if lower.contains("timos")
        || lower.contains("nokia")
        || lower.contains("7750 sr")
        || lower.contains("alcatel")
        || lower.contains("*a:")
        || (lower.contains("a:") && lower.contains("#"))
    {
        "Nokia SR OS"
    } else if lower.contains("arista") || lower.contains("eos") || lower.contains("veos") {
        "Arista EOS"
    } else if lower.contains("mikrotik") || lower.contains("routeros") {
        "MikroTik RouterOS"
    } else if lower.contains("fortigate") || lower.contains("fortios") {
        "Fortinet FortiOS"
    } else {
        "Linux Server / Bash"
    };

    let mut catalog = get_catalog_for_platform(platform);

    // Merge learned custom commands for this platform at the top
    let learned_map = load_learned_commands();
    if let Some(learned) = learned_map.get(platform) {
        for item in learned.iter().rev() {
            if !catalog.iter().any(|c| c.cmd == item.cmd) {
                catalog.insert(0, item.clone());
            }
        }
    }

    let filtered = filter_suggestions_by_prefix(&catalog, &typed_cmd);

    (platform.to_string(), typed_cmd, filtered)
}

pub async fn get_inline_autocomplete(
    banner: &str,
    recent_context: &str,
    provider: &str,
) -> Result<(String, Vec<AiAutocompleteSuggestion>), String> {
    let clean_banner = strip_ansi_codes(banner);
    let clean_recent = strip_ansi_codes(recent_context);
    let full_text = format!("{}\n{}", clean_banner, clean_recent);

    // Instant local heuristic platform detection & prefix-filtered subcommands
    let (detected_platform, typed_cmd, default_suggestions) = detect_platform_and_suggestions(&full_text);

    // Prepare a concise, high-speed LLM prompt (only last 15 lines of context)
    let recent_slice: Vec<&str> = clean_recent.lines().rev().take(15).collect();
    let compact_context = recent_slice.into_iter().rev().collect::<Vec<_>>().join("\n");

    let prompt = if !typed_cmd.is_empty() {
        format!(
            r#"Device Type: {}
User is currently typing: "{}"
Recent terminal context:
"""
{}
"""
Suggest 5-10 specific command completions or subcommands starting with or expanding "{}" for this device.
Return ONLY valid JSON in format:
{{"suggestions": [{{"cmd": "completed command", "desc": "short description"}}]}}"#,
            detected_platform, typed_cmd, compact_context, typed_cmd
        )
    } else {
        format!(
            r#"Device Type: {}
Recent terminal context:
"""
{}
"""
Suggest 5-10 relevant command completions for this device and recent context.
Return ONLY valid JSON in format:
{{"suggestions": [{{"cmd": "command", "desc": "short description"}}]}}"#,
            detected_platform, compact_context
        )
    };

    let messages = vec![AiMessage {
        role: "user".to_string(),
        content: prompt,
    }];

    // Run LLM chat with small payload
    match chat_with_model(provider, "", messages).await {
        Ok(res) => {
            let mut clean_json = res.trim().to_string();
            if clean_json.starts_with("```") {
                if let Some(start) = clean_json.find('{') {
                    if let Some(end) = clean_json.rfind('}') {
                        clean_json = clean_json[start..=end].to_string();
                    }
                }
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&clean_json) {
                let mut suggestions = Vec::new();
                if let Some(arr) = val.get("suggestions").and_then(|s| s.as_array()) {
                    for item in arr {
                        let cmd = item
                            .get("cmd")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string();
                        let desc = item
                            .get("desc")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !cmd.is_empty() {
                            suggestions.push(AiAutocompleteSuggestion { cmd, desc });
                        }
                    }
                }
                if !suggestions.is_empty() {
                    // Combine LLM completions with catalog subcommands without duplicates
                    let mut combined = suggestions;
                    for def in default_suggestions {
                        if !combined.iter().any(|s| s.cmd == def.cmd) {
                            combined.push(def);
                        }
                    }
                    return Ok((detected_platform, combined));
                }
            }
            Ok((detected_platform, default_suggestions))
        }
        Err(_) => Ok((detected_platform, default_suggestions)),
    }
}

pub fn get_default_autocomplete_suggestions() -> Vec<AiAutocompleteSuggestion> {
    get_catalog_for_platform("Linux Server / Bash")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_autocomplete_suggestions_not_empty() {
        let suggestions = get_default_autocomplete_suggestions();
        assert!(suggestions.len() > 10);
        assert!(!suggestions[0].cmd.is_empty());
        assert!(!suggestions[0].desc.is_empty());
    }

    #[test]
    fn test_platform_heuristics_detection() {
        let (p1, s1) = detect_platform_from_terminal("Cisco IOS Software, C3750 Software (C3750-IPSERVICESK9-M)");
        assert_eq!(p1, "Cisco IOS / IOS-XE");
        assert!(!s1.is_empty());

        let (p2, s2) = detect_platform_from_terminal("JUNOS 21.4R1.12 built by builder on 2021-12-16");
        assert_eq!(p2, "Juniper Junos OS");
        assert!(!s2.is_empty());

        let (p3, s3) = detect_platform_from_terminal("TiMOS-B-22.10.R1 both/x86_64 Nokia 7750 SR");
        assert_eq!(p3, "Nokia SR OS");
        assert!(!s3.is_empty());

        let (p4, s4) = detect_platform_from_terminal("Linux ubuntu-server 5.15.0-generic x86_64");
        assert_eq!(p4, "Linux Server / Bash");
        assert!(!s4.is_empty());

        let (p5, s5) = detect_platform_from_terminal("ADTRAN Total Access 5000 SCM-A (OLT-XGS)");
        assert_eq!(p5, "Adtran TA5000 OLT");
        assert!(s5.iter().any(|s| s.cmd == "show shelf"));
        assert!(s5.iter().any(|s| s.cmd == "show gpon ont"));

        let (p6, s6) = detect_platform_from_terminal("ADTRAN SDX 6320-16 10G Switch");
        assert_eq!(p6, "Adtran SDX Switch");
        assert!(s6.iter().any(|s| s.cmd == "show port-channel summary"));
    }

    #[test]
    fn test_subcommand_prefix_filtering() {
        let (platform, typed, suggestions) = detect_platform_and_suggestions(
            "Cisco IOS Software, Catalyst 3850\nSwitch# show int",
        );
        assert_eq!(platform, "Cisco IOS / IOS-XE");
        assert_eq!(typed, "show int");
        assert!(suggestions.iter().any(|s| s.cmd == "show interfaces status"));
        assert!(suggestions.iter().any(|s| s.cmd == "show ip interface brief"));

        let (platform2, typed2, suggestions2) = detect_platform_and_suggestions(
            "Linux ubuntu-2204\nubuntu@srv:~$ systemctl res",
        );
        assert_eq!(platform2, "Linux Server / Bash");
        assert_eq!(typed2, "systemctl res");
        assert!(suggestions2.iter().any(|s| s.cmd == "systemctl restart"));

        let (platform3, typed3, suggestions3) = detect_platform_and_suggestions(
            "ADTRAN Total Access 5000\nTA5000# show gp",
        );
        assert_eq!(platform3, "Adtran TA5000 OLT");
        assert_eq!(typed3, "show gp");
        assert!(suggestions3.iter().any(|s| s.cmd == "show gpon ont"));
        assert!(suggestions3.iter().any(|s| s.cmd == "show gpon ont status"));
    }

    #[test]
    fn test_learned_commands_store() {
        save_learned_command("Adtran TA5000 OLT", "show custom test command", "Special diagnostic test");
        let (platform, _, suggestions) = detect_platform_and_suggestions("ADTRAN Total Access 5000\nTA5000# ");
        assert_eq!(platform, "Adtran TA5000 OLT");
        assert!(suggestions.iter().any(|s| s.cmd == "show custom test command"));
    }

    #[tokio::test]
    async fn test_inline_autocomplete_fallback_when_offline() {
        let (detected, suggestions) = get_inline_autocomplete(
            "Welcome to Ubuntu 22.04 LTS",
            "root@server:~# show int",
            "invalid_provider",
        )
        .await
        .unwrap();

        assert_eq!(detected, "Linux Server / Bash");
        assert!(!suggestions.is_empty());
    }
}




